# Self-hosted runners

GitHub-hosted Actions minutes are not used. Builds run on two machines you register yourself.

英文说明在下面。中文在前。

## 中文

这两台机器只给私有仓库 `kurlez/newtype` 用。不要把它们注册成公共仓库的 runner，也不要打开 “Allow GitHub Actions to run on self-hosted runners for public repositories” 或 fork PR 的工作流。fork 来的 pull request 即使打开了，本仓库的 workflow 也会跳过。

拿一次性注册令牌（一小时内有效）。令牌只在脚本提示时粘贴，不要写进文件、不要发到聊天里：

```bash
gh api -X POST repos/kurlez/newtype/actions/runners/registration-token --jq .token
```

也可以在网页上打开仓库 Settings → Actions → Runners → New self-hosted runner，从配置命令里复制 token。不要执行网页上那条官方 `./config.sh`，用下面的脚本。

卸载要用另一枚 remove token：

```bash
gh api -X POST repos/kurlez/newtype/actions/runners/remove-token --jq .token
```

Windows（管理员 PowerShell）。只下载 `setup-windows.ps1` 到 `~/.newtype-runner-setup/` 再运行即可。默认服务账户是 `NT AUTHORITY\SYSTEM`，不需要密码。如果上次已经注册但没有服务，再运行一次会用 remove token 卸掉注册并重新配置。脚本把 `CARGO_HOME` 和 `RUSTUP_HOME` 写成它实际验证过的 rustup 目录，不沿用环境里失效的路径。要用自己的账户时再传 `-LogonAccount` 和 `-Credential`，脚本不会主动问密码。

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
~\.newtype-runner-setup\setup-windows.ps1
~\.newtype-runner-setup\setup-windows.ps1 -Uninstall
```

Mac 上如果没有 `~/.cargo/bin/rustup`，脚本会安装 rustup stable，不使用 Homebrew 里较旧的 rustc。脚本自带最低 rustc 版本，不读取仓库文件。`~/.cargo/bin` 会写在 runner 的 `.path` 和 `.env` 最前面。只下载这一个脚本到 `~/.newtype-runner-setup/` 再运行即可。

macOS（登录用户，不要用 sudo，在仓库目录里）：

```bash
bash scripts/runner/setup-mac.sh
bash scripts/runner/setup-mac.sh --uninstall
```

查看是否在线：

```bash
gh api repos/kurlez/newtype/actions/runners --jq ".runners[] | {name,status,busy,labels:[.labels[].name]}"
```

保持醒着，否则任务会停在半路。Mac 的 runner 是当前用户的 LaunchAgent，只在这个用户保持登录时运行。系统设置 → 能量，接电源时不要自动睡眠。Windows 用管理员执行：

```powershell
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
```

脚本不会把令牌或 Windows 密码写到磁盘，也不会把令牌打出来。Windows 服务设为自动启动。

## English

These machines are only for the private repository `kurlez/newtype`. Do not register them on a public repository, and do not enable self-hosted runners for public repositories or for pull requests from forks. A fork pull request is skipped by the workflow even if that setting is on.

Get a one-hour registration token. Paste it only at the script prompt. Do not store it in a file or send it in chat:

```bash
gh api -X POST repos/kurlez/newtype/actions/runners/registration-token --jq .token
```

The same token is shown under Settings → Actions → Runners → New self-hosted runner. Use the scripts below instead of the `./config.sh` line on that page.

Uninstall needs a remove token:

```bash
gh api -X POST repos/kurlez/newtype/actions/runners/remove-token --jq .token
```

Windows, from an elevated PowerShell. Download `setup-windows.ps1` by itself into `~/.newtype-runner-setup/` and run that file. The service account is `NT AUTHORITY\SYSTEM` and no password is requested. If a previous run registered the runner but did not create a service, run the script again: it removes that registration and configures the service. `CARGO_HOME` and `RUSTUP_HOME` in the runner `.env` are the rustup directories the script verified, not a stale value from the user environment. Pass `-LogonAccount` and `-Credential` only when the service should use a different account. The script does not prompt for a password.

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
~\.newtype-runner-setup\setup-windows.ps1
~\.newtype-runner-setup\setup-windows.ps1 -Uninstall
```

On the Mac, a missing `~/.cargo/bin/rustup` is installed as rustup stable. Homebrew's older rustc is not the compiler the runner uses. The script carries its own minimum rustc version and does not read the repo, so download that one file into `~/.newtype-runner-setup/` and run it from there. `~/.cargo/bin` is written first in the runner `.path` and `.env` files.

macOS, as the logged-in user, not root:

```bash
bash scripts/runner/setup-mac.sh
bash scripts/runner/setup-mac.sh --uninstall
```

Passing `--token` works, but the shell history keeps it. The hidden prompt does not.

Check the runners:

```bash
gh api repos/kurlez/newtype/actions/runners --jq ".runners[] | {name,status,busy,labels:[.labels[].name]}"
```

Keep the machines awake. The Mac runner is a LaunchAgent for the current user and runs only while that user stays logged in. In System Settings → Energy, turn off sleep on power adapter. On Windows, from an elevated PowerShell:

```powershell
powercfg /change standby-timeout-ac 0
powercfg /change hibernate-timeout-ac 0
```

The scripts do not write the token or a Windows password to disk, and they do not print tokens. The Windows service is set to start automatically.

Pull requests run tests and the Windows build. macOS and publish run on a push to `main`, a `v*` tag, or a manual run of those refs. Publish stays on the Windows runner and verifies updater signatures in Node, so it does not install minisign and does not use sudo.
