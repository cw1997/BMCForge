# BMCForge

使用 Rust + Ratatui 管理多台服务器的原生 IPMI 终端工具：查看风扇 RPM 和传感器、设置固定占空比、恢复 BMC 自动模式。无需安装 ipmitool。

当前为首版开发版本。运行要求是可交互的终端和支持 IPMI 2.0 LAN/RMCP+ suite 3 的 BMC。建议使用 Windows Terminal、macOS Terminal 或 Linux 的现代终端。跨 CPU 的发布目标不代表支持旧版 Windows。

## 快速开始

安装 [Rust stable](https://rustup.rs/) 后：

```sh
cargo run --locked -p bmcforge -- --demo
```

演示模式完全离线，使用 Dell、Supermicro、Lenovo 及冲突机器类型的模拟设备，不读取或保存真实服务器配置。默认发现范围 `192.0.2.20-192.0.2.23` 可演示唯一识别、歧义选择和最终添加预览。

```sh
cargo run --locked -p bmcforge -- --config bmcforge.local.toml init
cargo run --locked -p bmcforge -- --config bmcforge.local.toml
```

在界面按 `i` 批量导入，或按 `d` 发现 IPv4 网段。每行接受一个 IP、主机名、CIDR 或完整地址范围，例如 `192.0.2.10-192.0.2.20`。发现不接受主机名和 IPv6 网段。IPv4 CIDR 排除网络及广播地址，`/31`、`/32` 按点对点／单地址处理。

导入先预览，按 Enter 继续填写共同凭据，按 F2 保存。发现流程为无认证探测 → 选择设备与本批凭据 → 身份识别 → 处理型号歧义 → 最终预览确认添加；识别不要求 SDR 成功。普通导入可保存离线设备。无响应表示未知，不表示设备不存在。凭据不会自动轮换尝试。

## 终端操作

- `1` 至 `5` 或 Tab：服务器、传感器、网络发现、适配库、操作日志。
- ↑ / ↓ 或 j / k：移动；Enter：详情或应用选中的适配。
- Space：多选；Esc：取消对话框，或清空搜索和多选。
- `i`：导入；`d`：发现／取消发现；发现页 `f` 切换候选／全部结果。
- `e`：编辑当前服务器 TOML，包括独立凭据、适配、超时和轮询参数。
- `m`：固定占空比；`a`：请求自动模式；操作前列出服务器和参数，Enter 才执行。
- `p`：适配库；适配页 `u` 将公开默认凭据填入当前服务器的编辑表单，仍需保存。
- `/`：搜索服务器、地址和标签；`,`：编辑设置；PgUp/PgDn：滚动详情／日志。
- `x`：导出脱敏诊断到配置文件目录；`q` 或 Ctrl+C：恢复自动模式并退出。
- Delete：先恢复自动模式，再移除服务器；F1：操作帮助。
- 编辑框 F2 或 Ctrl+Enter：保存／继续；Esc：取消；支持多行粘贴。F2 适用于无法区分 Ctrl+Enter 的终端。
- 默认启用鼠标：点击页签、行、选择框、适配项、语言和操作按钮；双击服务器进入详情；滚轮只滚动指针所在区域。弹窗有可点击的确认／取消按钮。
- F3 或顶部语言入口：即时切换语言并保存；F4：即时切换鼠标。终端复制通常使用 Shift+拖动或终端原生选择方式。
- 发现页 A：全选可见候选；G：选择当前 /24 分组；`i`：识别所选；`c`：取消识别。型号选择可搜索，F5 保留未知、F6 跳过。

快捷键可通过 `[settings.keys]` 修改。服务器按第一个标签分组排序。启动只连接和监控，不重放之前的调速操作。

## 配置与适配

默认配置使用操作系统用户配置目录，通过 `--config` 指定路径可便携使用。参见 [示例配置](examples/config.toml) 和 [配置说明](docs/configuration.md)。密码按设计明文保存；配置保存采用同目录临时文件原子替换，并保留 `.bak`，损坏文件不会被自动覆盖。

适配采用 TOML，无需编译即可扩展：

```sh
cargo run --locked -p bmcforge -- --profiles-dir ./my-profiles validate-profiles
cargo run --locked -p bmcforge -- --profiles-dir ./my-profiles
cargo run --locked -p bmcforge -- --config bmcforge.local.toml validate-config
cargo run --locked -p bmcforge -- profile-schema
```

同 ID 外部适配需显式声明 `override_builtin = true`。适配库展示命令来源、固件限制、默认凭据适用范围和验证状态。所有附带 OEM 写适配目前均为实验性，需在服务器设置中明确启用 `allow_experimental = true`。参见 [适配开发](docs/profiles.md) 和 [兼容范围](docs/compatibility.md)。

新配置默认英文，内置 11 种语言，保留已有语言与鼠标偏好。使用 `--locales-dir` 加载 Fluent 社区包，`validate-locales` 校验；见 [语言包贡献指南](docs/localization.md)。型号目录与写命令分离，社区型号位于 `--profiles-dir` 下的 `models/`，使用 `validate-models` 校验；见 [识别流程与 Lenovo 分组控制](docs/models.md)。Lenovo x3650 M5 IMM2 提供实验性原始档位，不推算百分比；其他 System x 型号只识别和监控。

实际 RPM 来自传感器；请求的占空比单独显示。当前占空比没有读回实现，因此显示 unknown。写操作超时不会重放命令，状态标为不确定；正常退出会对本次接管过的设备尝试恢复自动控制。强杀、断电、网络故障时无法保证恢复。此版本没有温控曲线或后台保护服务。

目录已扩展到 164 项：新增 HPE、ThinkSystem、浪潮、新华三、华为、超聚变、曙光、中兴及中国长城的代表性型号识别与监控。H3C UIS 2000 G6 提供经过只读协议探测的独立实验性调速适配；识别、监控和调速范围分别记录，不承诺整个品牌都可调速。参见 [厂商覆盖与依据](docs/vendor-coverage.md)。用 `list-models --vendor HPE` 或 `list-models --vendor 曙光 --json` 查看目录。

## 开发与验证

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --manifest-path vendor/ipmi/Cargo.toml --locked --no-default-features --features blocking --lib
cargo run --locked -p bmcforge -- demo-check
python -m unittest discover -s tests -v
cargo build --release --locked -p bmcforge
```

工作区测试包含命令级模拟 BMC 和 TUI TestBackend；vendor 测试使用 UDP 回环 BMC 验证 RAKP、加密通信、完整性校验、错误密码、重放和写超时。模拟测试不是实机验证。参见 [实机测试流程](docs/hardware-testing.md)。

GitHub Actions 为 Windows x64/ARM64/x86、macOS Intel/Apple Silicon、Linux musl x64/ARM64/ARMv7 构建、测试并打包。Linux ARM 用 cross + QEMU；macOS 使用 macOS runner；Windows 使用 MSVC。每包包含测试方式说明和 SHA-256。参见 [发布流程](docs/releasing.md)。

## 贡献与许可证

新增适配请提交来源、明确的型号和固件范围、恢复自动模式的方法和命令测试样例，遵循 [贡献说明](CONTRIBUTING.md)。不将品牌相同或支持 IPMI 当作可调速的依据。

BMCForge 使用 [Apache-2.0](LICENSE)。原生会话实现基于仓库内维护的 ipmi 0.1.2，见 [第三方来源和修改](vendor/ipmi/BMCFORGE-PATCHES.md)。
