# 国内外服务器识别与控制范围

本轮目录审核日期：2026-10-16。型号目录共 164 项，新增 96 项；包含有来源的名称、别名、厂商编号、机器类型及适配关联。`docs/compatibility.json` 由目录和适配文件生成，记录每个型号的来源及控制状态。全部新增项目都没有 BMCForge 实机验证；识别、监控和调速分别判断。

## 已编入目录的厂商

- **HPE / HP**：18 项，ProLiant DL360p/DL380p Gen8、DL360/DL380 Gen9–11，以及 ProLiant Compute Gen12 的 DL320/DL340/DL360/DL380/DL380a/DL384/DL580/ML350。支持 HP/HPE 名称别名。`hpe-ilo-monitor` 使用标准 IPMI 读取设备提供的传感器；部分 iLO 风扇数据是百分比或状态，不会伪装为 RPM。
- **Lenovo / 联想**：保留 12 项 System x M3/M4/M5，增加 27 项 ThinkSystem，包括 SR530/550/570/590、SR630/650 V1–V4、AMD SR635/645/655/665 及 V3、SR850/860/950、ST550/ST650 V2/V3。支持官方索引中的字母数字机器类型，如 7D75。共享机器类型（如 SR650 V4 与 SR650a V4）保留多候选。XCC 不使用 IMM2 风扇组命令。
- **Inspur / 浪潮**：5 项，NF5270M4、NF5280M5、NF5180M6、NF5280M6、NF5280M7；兼容有明确型号前缀的 IEIT SYSTEMS 名称。`inspur-monitor` 提供标准 IPMI 监控。
- **H3C / 新华三**：12 项 UniServer HDM 机型，包含 R4700/R4900/R6900/R4300 的 G5/G6，以及 R4900 G6 Ultra、R4950/R6700/R4500 G6；另有 UIS 2000 G6 独立型号与实验性调速适配。不能把 UIS OEM 命令用于整个 H3C 品牌。
- **Huawei / 华为**：14 项，RH1288/RH2288/RH2288H/RH5885H/RH8100 V3、5288 V3、2288/2288C/2488/5885H/8100 V5，以及 TaiShan 200 1280/2280/5280。`huawei-ibmc-monitor` 提供标准 IPMI 监控。
- **xFusion / 超聚变**：6 项，FusionServer 1288H/2288H V5/V6/V7。历史华为与超聚变命名映射到同一个物理型号条目，保留原始厂商证据，不制造两个虚假物理型号。`xfusion-ibmc-monitor` 提供标准 IPMI 监控。
- **Sugon / Dawning / 曙光**：3 项，I620-G20/G30/G40，含中英文别名。`sugon-monitor` 提供标准 IPMI 监控。
- **ZTE / 中兴通讯**：5 项，R5200/R5300/R5500/R6500/R8500 G5，提供标准 IPMI 监控。
- **China Greatwall / 中国长城**：5 项，擎天 RF5260 V5、RF6260 V5、DF729、EF860、EF862，提供标准 IPMI 监控。

原有 56 项 Dell PowerEdge 型号以及 Supermicro、ASRock 控制适配保持独立。未列出的型号仍可使用 `generic` 监控；前提是设备实际支持并启用了本程序支持的 IPMI LAN/cipher suite。目录不承诺所有型号、OEM 贴牌平台和固件都提供同一种协议。

## 新增 H3C UIS 实验性调速

适配依据 [H3C UIS 2000 G6 IPMI OEM 命令手册](https://www.h3c.com/cn/Service/Document_Software/Document_Center/H3Cloud/Catalog/H3C_UIS/2000_G6/Developer_Guides/API_Reference/H3C_UIS_2000_G6_IPMI_OEM-15014/)。需要 `allow_experimental = true`，并且实时身份必须匹配 UIS 2000 G6；UniServer 型号不匹配。

文档列出了五个 PWM 对应点：20/40/60/80/100% 对应 0x33/0x66/0x99/0xcc/0xff。只开放这五个值。首次接管先以 Get Fan Mode 检查 `aa e5 00 00` 响应；字节序、签名、当前模式或 API 不符合时，不尝试其他前缀，也不启动风扇写入。文档中的 1.01.07 不能从标准 Device ID 的两段版本直接推断，因此以身份限制与实际协议探测结合，不能宣传为所有固件兼容。

手动模式、PWM 设置和恢复自动模式分别检查响应。写入结果不确定时保留恢复义务，不盲目重放。具体请求、响应样例在适配文件及模拟 BMC 测试中。模拟通过不代表实机通过。

## 仍需控制依据的接口

[HPE 社区控制实现](https://github.com/alex3025/ilo-fans-controller) 使用修改版 iLO 的 SSH 接口，超出本程序当前纯 IPMI 范围。HPE 型号识别成功不会开放该 SSH 控制。

[H3C HDM 文档](https://www.h3c.com/en/Support/Resource_Center/EN/Home/Public/00-Public/Technical_Documents/Developer_Documents/API_References/H3C_CR-13775/202401/2017531_294551_0.htm) 的风扇策略／重载配置，与 UIS 的手动 PWM 不是同一协议。[浪潮 BMC 文档](https://en.inspur.com/eportal//fileDir/active_download/platformBookZh/6680/Inspur%C2%A0Server%C2%A0BMC%C2%A0User%C2%A0Manual%C2%A0V2.7.pdf) 提供 WebUI 风扇控制，不能据此复制 Dell raw 命令。华为／超聚变的 CLI fanmode/fanlevel、曙光的特定基准平台全速命令，也不能替代有明确恢复语义的本型号 IPMI 控制协议。中兴、长城没有在本轮找到可确认型号和恢复行为的完整 raw 命令合同，因此保持监控。

每个目录项提供产品资料来源；厂商编号参考 [IANA PEN 注册表](https://www.iana.org/assignments/enterprise-numbers.txt)。PEN 注册本身不证明某个 BMC 一定返回此编号。OEM/ODM 返回其他编号时保留未知，允许社区按脱敏 FRU 和 Device ID 增补。仅有厂商品牌或产品宣传页，不会开放写命令；本轮也不增加未经证实的通用密码。

## 查看和贡献

```sh
bmcforge validate-models
bmcforge validate-profiles
bmcforge list-models --vendor HPE
bmcforge list-models --vendor 曙光 --json
bmcforge --profiles-dir ./community validate-models
```

适配库详情显示关联型号；命令行可按中英文厂商名称筛选。完整清单以 TOML／生成的 JSON 为准，持续维护，不宣称穷尽所有服务器。提交新型号需要身份来源；提交调速还需要精确型号／固件或协议探测限制、取值编码、读取／设置／恢复依据及失败恢复测试。参见 [贡献说明](../CONTRIBUTING.md)、[适配格式](profiles.md) 和 [实机测试指南](hardware-testing.md)。
