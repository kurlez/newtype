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

Windows（管理员 PowerShell，在仓库目录里）：

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\runner\setup-windows.ps1
.\scripts\runner\setup-windows.ps1 -Uninstall
```

Mac 上如果没有 `~/.cargo/bin/rustup`，脚本会安装 rustup stable，不使用 Homebrew 里较旧的 rustc。`src-tauri/Cargo.toml` 的 `rust-version` 是最低版本。`~/.cargo/bin` 会写在 runner 的 `.path` 和 `.env` 最前面。上次如果停在 host 检查，重新运行脚本即可。

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

如果 Windows 服务起不来，在 `secpol.msc` 里给这个账户加上 “Log on as a service”，再运行一次脚本。脚本不会把令牌或 Windows 密码写到磁盘。

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

Windows, from an elevated PowerShell in the repo:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
.\scripts\runner\setup-windows.ps1
.\scripts\runner\setup-windows.ps1 -Uninstall
```

On the Mac, a missing `~/.cargo/bin/rustup` is installed as rustup stable. Homebrew's older rustc is not the compiler the runner uses. `rust-version` in `src-tauri/Cargo.toml` is the minimum. `~/.cargo/bin` is written first in the runner `.path` and `.env` files. Re-run the script if an earlier run stopped at the host check.

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

If the Windows service does not start, grant the account “Log on as a service” in `secpol.msc` and run the script again. The scripts do not write the token or the Windows password to disk.

Pull requests run tests and the Windows build. macOS and publish run on a push to `main`, a `v*` tag, or a manual run of those refs. Publish stays on the Windows runner and verifies updater signatures in Node, so it does not install minisign and does not use sudo.
