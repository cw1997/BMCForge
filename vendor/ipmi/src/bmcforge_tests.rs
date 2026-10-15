//! Local UDP protocol tests added by BMCForge; no hardware or real credentials.
use crate::{BlockingClient, PrivilegeLevel, protocol::*};
use std::{
    net::{SocketAddr, UdpSocket},
    thread,
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Normal,
    BadIntegrity,
    NoSecurity,
    Replay,
    Silent,
}

fn receive(socket: &UdpSocket) -> (Vec<u8>, SocketAddr) {
    let mut data = [0u8; 4096];
    let (len, peer) = socket.recv_from(&mut data).expect("mock receives packet");
    (data[..len].to_vec(), peer)
}
fn send(socket: &UdpSocket, peer: SocketAddr, payload_type: u8, payload: &[u8]) {
    socket
        .send_to(
            &encode_rmcpplus_packet(payload_type, 0, 0, payload, false, false, None).unwrap(),
            peer,
        )
        .unwrap();
}
fn checksum(bytes: &[u8]) -> u8 {
    0u8.wrapping_sub(bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b)))
}

fn bmc(
    mode: Mode,
    password: &'static str,
    bad_password_expected: bool,
) -> (SocketAddr, thread::JoinHandle<Vec<u8>>) {
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let address = socket.local_addr().unwrap();
    let task = thread::spawn(move || {
        let sid = 0x12345678u32;
        let (data, peer) = receive(&socket);
        let packet = decode_rmcpplus_packet(&data, None).unwrap();
        assert_eq!(packet.payload_type, payload_type::OPEN_SESSION_REQUEST);
        let p = packet.payload;
        assert_eq!([p[12], p[20], p[28]], [1, 1, 1]);
        let remote = u32::from_le_bytes(p[4..8].try_into().unwrap());
        let mut open = vec![p[0], 0, 4, 0];
        open.extend(remote.to_le_bytes());
        open.extend(sid.to_le_bytes());
        open.extend_from_slice(&p[8..32]);
        send(&socket, peer, payload_type::OPEN_SESSION_RESPONSE, &open);
        let (data, _) = receive(&socket);
        let packet = decode_rmcpplus_packet(&data, None).unwrap();
        assert_eq!(packet.payload_type, payload_type::RAKP_1);
        let p = packet.payload;
        let console_random: [u8; 16] = p[8..24].try_into().unwrap();
        let bmc_random = [0x42u8; 16];
        let guid = [0x24u8; 16];
        let username = &p[28..];
        let key = user_key_sha1_from_secret(password.as_bytes());
        let auth = rakp2_key_exchange_auth_code_sha1(
            &key,
            remote,
            sid,
            &console_random,
            &bmc_random,
            &guid,
            PrivilegeLevel::Administrator,
            username,
        )
        .unwrap();
        let mut rakp2 = vec![p[0], 0, 0, 0];
        rakp2.extend(remote.to_le_bytes());
        rakp2.extend(bmc_random);
        rakp2.extend(guid);
        rakp2.extend(auth);
        send(&socket, peer, payload_type::RAKP_2, &rakp2);
        if bad_password_expected {
            return vec![];
        }
        let sik = compute_sik_sha1(
            &key,
            &console_random,
            &bmc_random,
            PrivilegeLevel::Administrator,
            username,
        )
        .unwrap();
        let security = derive_security_context_sha1(&sik).unwrap();
        let (data, _) = receive(&socket);
        let packet = decode_rmcpplus_packet(&data, None).unwrap();
        assert_eq!(packet.payload_type, payload_type::RAKP_3);
        let expected = rakp3_key_exchange_auth_code_sha1(
            &key,
            &bmc_random,
            remote,
            PrivilegeLevel::Administrator,
            username,
        )
        .unwrap();
        assert_eq!(packet.payload[8..], expected);
        let icv = rakp4_integrity_check_value_sha1_96(&sik, &console_random, sid, &guid).unwrap();
        let mut rakp4 = vec![packet.payload[0], 0, 0, 0];
        rakp4.extend(remote.to_le_bytes());
        rakp4.extend(icv);
        send(&socket, peer, payload_type::RAKP_4, &rakp4);
        let mut commands = vec![];
        for seq in 1..=3 {
            let (data, _) = receive(&socket);
            let packet = decode_rmcpplus_packet(&data, Some(&security)).unwrap();
            assert!(packet.is_authenticated && packet.is_encrypted);
            let request = packet.payload;
            let netfn = request[1] >> 2;
            let command = request[5];
            commands.push(command);
            if mode == Mode::Silent {
                thread::sleep(Duration::from_millis(650));
                socket.set_nonblocking(true).unwrap();
                let mut buf = [0u8; 4096];
                assert!(
                    socket.recv_from(&mut buf).is_err(),
                    "write was retransmitted"
                );
                return commands;
            }
            let payload = if command == 1 {
                vec![1, 0, 2, 0x30, 2, 0, 0xa2, 2, 0, 1, 0, 0, 0, 0, 0]
            } else {
                vec![]
            };
            let mut response = vec![0x81, (netfn + 1) << 2, 0, 0x20, request[4], command, 0];
            response[2] = checksum(&response[..2]);
            response.extend(payload);
            response.push(checksum(&response[3..]));
            let secure = mode != Mode::NoSecurity;
            let encoded = if secure {
                encrypt_payload_aes_cbc(&response, &security.aes_key, &[7; 16]).unwrap()
            } else {
                response
            };
            let mut response = encode_rmcpplus_packet(
                0,
                remote,
                if mode == Mode::Replay { 1 } else { seq },
                &encoded,
                secure,
                secure,
                Some(&security),
            )
            .unwrap();
            if mode == Mode::BadIntegrity {
                let end = response.len() - 1;
                response[end] ^= 1;
            }
            socket.send_to(&response, peer).unwrap();
            if mode == Mode::BadIntegrity
                || mode == Mode::NoSecurity
                || (mode == Mode::Replay && seq == 2)
                || command == 0x3c
            {
                return commands;
            }
        }
        commands
    });
    (address, task)
}
fn client(addr: SocketAddr, password: &str) -> crate::Result<BlockingClient> {
    BlockingClient::builder(addr)
        .username("tester")
        .password(password)
        .timeout(Duration::from_millis(500))
        .retries(1)
        .build()
}

#[test]
fn encrypted_authenticated_udp_roundtrip_and_reconnect() {
    for _ in 0..2 {
        let (addr, task) = bmc(Mode::Normal, "secret", false);
        let c = client(addr, "secret").unwrap();
        assert_eq!(c.get_device_id().unwrap().manufacturer_id, 674);
        c.close_session().unwrap();
        assert_eq!(task.join().unwrap(), [1, 0x3c]);
    }
}
#[test]
fn wrong_password_rejected_during_rakp() {
    let (addr, task) = bmc(Mode::Normal, "correct", true);
    assert!(matches!(
        client(addr, "wrong"),
        Err(crate::Error::AuthenticationFailed(_))
    ));
    task.join().unwrap();
}
#[test]
fn tampering_and_unsecured_responses_are_rejected() {
    for mode in [Mode::BadIntegrity, Mode::NoSecurity] {
        let (addr, task) = bmc(mode, "secret", false);
        let c = client(addr, "secret").unwrap();
        assert!(c.get_device_id().is_err());
        assert_eq!(task.join().unwrap(), [1]);
    }
}
#[test]
fn duplicate_incoming_sequence_rejected() {
    let (addr, task) = bmc(Mode::Replay, "secret", false);
    let c = client(addr, "secret").unwrap();
    c.get_device_id().unwrap();
    assert!(c.get_device_id().is_err());
    assert_eq!(task.join().unwrap(), [1, 1]);
}
#[test]
fn write_timeout_is_not_retried() {
    let (addr, task) = bmc(Mode::Silent, "secret", false);
    let c = client(addr, "secret").unwrap();
    assert!(matches!(
        c.send_raw(0x30, 0x30, &[1, 0]),
        Err(crate::Error::Timeout)
    ));
    assert_eq!(task.join().unwrap(), [0x30]);
}
