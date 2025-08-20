# RetroGameSearch

一个基于 Rust + egui 的桌面应用，用于从 `xmldb/` 文件夹读取大量游戏数据库 XML，并提供本地快速搜索与查看。

## 环境要求
- Rust（1.73+ 建议）：
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source $HOME/.cargo/env
```
- macOS/Linux/Windows 均可运行（已在 macOS 上开发）

## 快速开始
1. 将 XML 数据放入项目根目录的 `xmldb/` 文件夹
2. 运行应用：
```bash
cargo run --release
```

## 字体与中文
- 应用启动时会尝试从系统中加载中文字体（优先 PingFang、Noto Sans CJK、Source Han Sans 等）

## 数据解析说明
- 每个 `<game>` 节点解析字段：
  - 名称：`<game name="...">`
  - 平台：由文件名推断（如 `Apple - IIGS ...xml`）
  - 区域与语言：优先级 `archive.region/languages` → `game.region/languages` → `details.region`
  - 归档名：`archive@name`
  - 源定位：记录对应 XML 文件路径与第几个 `<game>` 节点索引，方便提取原始 XML
- 支持自闭合标签（`<archive .../>`, `<details .../>`）

## 开发脚本
- fmt + clippy（建议）
```bash
cargo fmt
cargo clippy --all-targets --all-features -D warnings
```

## 目录结构
```
RetroGameSearch/
  ├─ src/
  │   ├─ main.rs        # UI、搜索/筛选、详情窗口
  │   └─ xml.rs         # XML 解析与 <game> 源片段提取
  ├─ xmldb/             # 放置 XML 数据（已包含示例）
  ├─ Cargo.toml
  └─ README.md
```

## 许可证
- MIT
