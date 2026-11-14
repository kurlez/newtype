# newtype

[English](README.md) · [简体中文](README.zh-CN.md)

macOS 与 Windows 上的 AI 语音输入法。

按住或点按快捷键即可口授。newtype 把语音转成文字，按需要整理措辞，再粘贴回你正在使用的应用。同一条路径也可以翻译，或处理选中的文字。可以使用云端服务，也可以使用本地模型。newtype 与 Typeless 没有关联，也未获其认可。

## 它能做什么

- **口授。** 右 Ctrl 开始和结束录音。默认是切换模式。按住说话在设置里。Esc 取消。
- **润色。** 默认整理会把句子说顺，但不会回答或执行请求。逐字模式更贴近转写。个人词典会在文本进入模型之前替换词条。
- **翻译。** 另一组快捷键把内容译成你设定的语言（默认英语）。
- **命令。** 第三组快捷键在有选区时处理选中的文字，没有选区时把口授当成写作请求。翻译和命令需要云端模型，或者至少 4B 的本地改写模型。
- **自动路由。** 默认在云端供应商能连通时走云端，一直连不上就改用本地模型，连接稳定后再回到云端。仅云端从不用本地模型。仅本地一直留在这台电脑上。严格离线还会阻止探测、更新检查和模型下载。
- **本地模型。** 语音用 Qwen3-ASR，改写用 Qwen3，本地路径用 Silero VAD 去掉静音，运行时是 sherpa-onnx 和 llama.cpp。权重在你选择下载时才获取，不在安装包里。
- **云端预设。** 新安装默认是 Groq。也包括 OpenAI、火山引擎、通义（百炼 DashScope），以及自定义的 OpenAI 兼容接口。
- **历史。** 记录只留在这台电脑上。默认保留 30 天，并且只留最新的 300 条。
- **Windows 任务栏条。** 录音时，状态条可以放在时钟左边的空位。空位不够就改为浮窗。macOS 使用浮窗。两者都可以隐藏。
- **更新。** 新安装检查稳定通道，在后台下载，重启前会询问。可以在设置里改到 dev 通道。已经保存过的通道不会被改写。

## 截图

仓库里还没有产品截图。

<!-- TODO: 对外宣传前补上产品截图。
     建议：设置窗口、Windows 任务栏条、历史。
     src-tauri/icons 和 app-icon.png 是应用图标，不是截图。
     不要放占位假图。 -->

## 下载

从稳定发布页安装当前版本：

**[github.com/kurlez/newtype-releases/releases/latest](https://github.com/kurlez/newtype-releases/releases/latest)**

| 平台 | 文件 |
| --- | --- |
| Windows 64 位 | `newtype_<ver>_x64-setup.exe` |
| Mac，Apple Silicon | `newtype_<ver>_aarch64.dmg` |

`<ver>` 是该发布的版本号，例如 `newtype_0.2.0_x64-setup.exe`。没有 Intel Mac 构建，也没有 Linux 安装包。

`.dmg` 和 Windows 安装程序用于第一次安装。之后的更新在应用内完成。更新器使用带签名的 `.app.tar.gz`（Mac）和带签名的 `-setup.exe`（Windows），不用 `.dmg`。

## 环境要求

- **macOS 12 或更新**，Apple Silicon。
- **Windows 10 或更新**，64 位。
- 麦克风。
- 云端口授需要网络，以及所选供应商的 API 密钥。
- 本地模型需要数 GB 磁盘空间。内存 16 GB 及以上的 Mac 使用 Qwen3-ASR 1.7B 和 Qwen3 4B。Windows，以及低于这个内存的 Mac，使用 Qwen3-ASR 0.6B 和 Qwen3 1.7B。体积和校验和见 [docs/local-models.md](docs/local-models.md)。

## 安装

这些构建没有公证，也没有 Apple Developer ID 或 Windows Authenticode 签名。

### macOS

1. 打开 `.dmg`，把 **newtype** 拖到「应用程序」。
2. 第一次不要双击。Gatekeeper 会提示无法验证开发者。
3. 在 Finder 中右键（或按住 Control 点击）该应用 → **打开**，再在对话框里点 **打开**。
4. 如果已经双击失败：打开 **系统设置 → 隐私与安全性**，找到被拦截的提示，点 **仍要打开**。可能需要管理员密码。
5. 之后从同一路径启动通常不再询问，直到新的签名身份让系统把它当成另一个程序。更新后可能需要重新允许麦克风和辅助功能。

### Windows

安装程序按当前用户安装。

1. 运行 `newtype_<ver>_x64-setup.exe`。
2. 如果 SmartScreen 显示“Windows 已保护你的电脑”，点 **更多信息**，再点 **仍要运行**。

## 第一次使用

1. 允许麦克风。在 macOS 上同时允许 **辅助功能**。如果快捷键没有反应，再允许 **输入监视**。设置窗口里有打开对应系统页面的按钮。
2. 默认供应商是 **Groq**。打开设置，粘贴 API 密钥，点 **保存密钥**。密钥写入 macOS 钥匙串或 Windows 凭据管理器，窗口在读回成功后才会说已保存。密钥不在 `dictation.json` 里。
3. 切到其他应用。按 **右 Ctrl** 开始，说完再按一次 **右 Ctrl**。newtype 粘贴文字，然后恢复原来的剪贴板。macOS 上粘贴是 Command+V，Windows 上是 Ctrl+V。
4. 可选：添加词典、把路由改成仅本地或严格离线，或下载本地模型。严格离线不能下载，要先下好再切换。

## API 密钥

在供应商控制台创建密钥，再粘贴到设置里。**加载模型**会用已保存的密钥调用 `GET /v1/models`。下表是新安装建议的模型 id，也可以自己填写。

| 供应商 | 语音 | 改写 | 密钥在哪里创建 |
| --- | --- | --- | --- |
| Groq（默认） | `whisper-large-v3-turbo` | `openai/gpt-oss-20b` | [console.groq.com/keys](https://console.groq.com/keys) |
| OpenAI | `gpt-transcribe` | `gpt-4o-mini` | [platform.openai.com/api-keys](https://platform.openai.com/api-keys) |
| 火山引擎 | openspeech `bigmodel` | `doubao-seed-2-1-lite-260915`（pro：`doubao-seed-2-1-pro-260915`） | 语音：[豆包语音控制台](https://console.volcengine.com/speech/app)。改写：[火山方舟 Ark](https://console.volcengine.com/ark)。 |
| 通义 · 百炼 DashScope | `qwen-audio-3.1-asr-flash` | `qwen3.8-flash`（也可以 `qwen3.8-max`） | [百炼控制台](https://bailian.console.aliyun.com/)。粘贴以 `/compatible-mode/v1` 结尾的 base URL。 |
| OpenAI 兼容 | 自己填写 | 自己填写 | 你自己的接口。使用 `https`，localhost 可以用 `http`。 |

**Groq。** 一把密钥同时用于语音和改写。改写使用 `reasoning_effort: low`，这是 `openai/gpt-oss-20b` 接受的最低档。另有一套「Groq + GPT-4o mini」：语音用 Groq 密钥，改写用 OpenAI 密钥，两把密钥分开存放。

**火山引擎。** 语音走 openspeech（资源 `volc.seedasr.sauc.duration`，模型名 `bigmodel`）。新版控制台保存语音 API Key。旧版控制台保存 App ID 和 Access Token。改写密钥是方舟的 Key，填在火山的 API key 里。思考会被关闭。不使用 `doubao-seed-asr-1-0`。

**通义 / 百炼。** 语音是 Qwen-Audio ASR，走 Model Studio 的多模态生成接口，词典词条作为即时热词发送。粘贴以 `/compatible-mode/v1` 结尾的业务空间地址，例如 `https://dashscope.aliyuncs.com/compatible-mode/v1`，或 `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`。改写使用同一 base 上的对话接口，并请求关闭思考。这条路径是 HTTP，不是实时 WebSocket 消息模型。

应用里的套餐按每天 200 次口授估算：Groq 约每月 1.8 美元，Groq 加 GPT-4o mini 约 2.5 美元，OpenAI 约 8 美元。数字核对于 2026-11-12，只是估计，不是账单。

## 本地模型

只想在云端不可用时兜底，就保持 **自动**。文件装好之后再切到 **仅本地** 或 **严格离线**。在设置里下载建议的语音模型和改写模型。Silero VAD 随语音模型安装，在本地识别前去掉静音。如果这个文件缺失，整段录音仍在本机识别，不会因此被发送出去。

低于 4B 的本地改写模型不能翻译，也不能执行命令。16 GB 及以上的 Mac 默认改写模型是 Qwen3 4B，这些模式可以在本地跑。Windows 默认改写模型是 Qwen3 1.7B，所以翻译和命令仍走云端，除非你另外安装 4B 模型。

严格离线会阻止下载。先在自动或仅本地下载，再切换。细节、校验和与许可证见 [docs/local-models.md](docs/local-models.md)。

## 默认快捷键

macOS 和 Windows 相同。左右修饰键是不同的键。保存的组合如果是空的，就使用默认值。

| 动作 | 默认 |
| --- | --- |
| 口授 | 右 Ctrl（`ControlRight`，不是左 Ctrl） |
| 翻译 | 右 Ctrl + 右 Shift |
| 命令 | 右 Ctrl + Enter |
| 取消录音 | Esc |

默认是切换：按一次开始，再按一次停止。按住说话在松开组合键时停止。

有的键盘没有右 Ctrl。默认值仍然是右 Ctrl。打开 **快捷键**，点 **捕获**，按一个键盘上存在的组合（右 Alt 可以），再点 **设置**。设置会立刻写入并开始使用。捕获过程中点取消或按 Esc，原来的组合保持不变。和其他应用的快捷键冲突（包括 Typeless 也在用的组合）只是警告。设置仍会保存这个组合。

## 隐私

- **API 密钥**在 macOS 钥匙串或 Windows 凭据管理器里。`dictation.json` 不含密钥。启动时会写入并删除一条一次性凭据。如果这次检查失败，设置页会说明存储不可用。
- **没有遥测。** newtype 不会上报使用统计。
- **日志**是应用日志目录里滚动的 `newtype.log`。设置页和托盘菜单会显示路径。一行只记元数据：阶段、路由、模型 id、字符数和错误文本。不记录转写，也不记录音频。写入前会去掉形如 API 密钥的文本，包括 `sk-…`、Groq 的 `gsk_…`、火山的 `AKLT…`、阿里云的 `LTAI…`，以及其他较长的令牌样式字符串。
- **云端。** 走云端路径时，录音发给你选的语音供应商，转写和提示词（包括当作提示的词典词条）发给改写供应商。自动模式在供应商能连通时会这样做。仅本地不会把语音或转写发给云端。严格离线还会阻止连通性探测、更新检查和模型下载。本地 llama.cpp 走 `127.0.0.1`，严格离线时仍然可用。
- **历史**是这台电脑上的明文 JSON。默认保留 30 天，并且只留这段时间里最新的 300 条。设置 → 历史 → **不保留** 之后不再写入新记录。**清空**会删掉已有记录。如果云端步骤失败，而且这段录音无法在本机做完，该条历史旁边可能会留下 wav。删除这条历史时会一起删除 wav。历史里的音频不会写进日志。
- **严格离线**表示不要离开这台电脑。它不会自动删掉已经存在的历史。

## 常见问题

**快捷键没反应。** 在 macOS 上先允许辅助功能，仍不行再允许输入监视。两边都要确认麦克风权限，以及你按的就是已保存的组合。左 Ctrl 不是默认键。

**键盘没有右 Ctrl。** 用捕获，再点设置。和其他应用冲突的警告不会阻止保存。

**macOS 提示无法验证开发者。** 按上面的右键 → 打开。不要指望第一次双击能通过。

**SmartScreen 拦截安装程序。** 更多信息 → 仍要运行。

**没有更新。** 新安装走 **稳定** 通道，只有打了版本标签才会前进。main 分支的构建在 **dev** 通道。切换通道不会降级。严格离线会关掉更新检查。升级不会改写 `settings.json` 里已经保存的通道。

**翻译或命令没有插入。** 这两种模式需要云端改写模型，或至少 4B 的本地模型。结果被留下时，转写仍在历史里。

**日志在哪？** 设置和托盘菜单会显示 `newtype.log` 的路径。里面是元数据，不是转写。

## 从源码构建

需要 Node 22、Rust stable（见 `rust-toolchain.toml`），以及 [Tauri 文档](https://tauri.app/start/prerequisites/)里列出的桌面构建库。

```bash
npm install
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run app:dev
```

`npm run app:dev` 是支持的开发命令。窗口会在启动时打开。关掉窗口是最小化。退出在托盘菜单里。Linux 可以用这种方式看界面。CI 不提供 Linux 安装包。

本地安装包还需要在环境变量里提供更新签名密钥。不要提交这把密钥。发布步骤在 [docs/RELEASING.md](docs/RELEASING.md)。

## 参与贡献

欢迎提 issue 和 pull request。测试、提示词回归，以及必须在真实 Mac 或 Windows 上做的检查，见 [CONTRIBUTING.md](CONTRIBUTING.md)。版本号、签名和发布流程见 [docs/RELEASING.md](docs/RELEASING.md)。

## 许可证

newtype 以 MIT 或 Apache-2.0 双许可发布，你可以任选其一。见 [LICENSE-MIT](LICENSE-MIT) 和 [LICENSE-APACHE](LICENSE-APACHE)。版权所有 2026 Wenfeng Liu。

其他许可证——Rust crate（包括 MPL-2.0 的 cssparser、selectors、option-ext、dtoa-short）、npm 包、下载的模型、FLEURS 样本，以及 OpenTypeless 提示词署名——汇总在 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

newtype 与 Typeless 没有关联，也未获其认可。
