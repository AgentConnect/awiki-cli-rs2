# Daemon 发布说明

这个目录只负责发布 `awiki-deamon` 的 Linux 下载包和安装脚本。

## 唯一发布入口

正常发布只执行这个脚本：

```bash
scripts/release/daemon/publish-linux.sh --base-url https://example.com
```

脚本应在拥有 Nginx 静态文件目录写入权限的 Ubuntu 服务器上执行。当前只发布
Linux amd64 包。

文件名以 `_` 开头的脚本是内部 helper，由 `publish-linux.sh` 调用。它们保留可
执行权限，方便排查发布流程，但日常发布不直接调用。

## 发布前准备

发布前按顺序完成：

1. 修改 `crates/awiki-deamon/Cargo.toml` 中的 daemon 版本号。
2. 同步 `Cargo.lock`，确保其中的 `awiki-deamon` package 版本一致。
3. 在包含本次待发布代码的 checkout 中执行发布脚本。

发布脚本会读取当前 checkout 的版本号，并校验它必须大于已发布
`releases/manifest.json` 里的 `latest`。版本没有递增时，脚本会拒绝发布。

只检查发布计划，不构建、不写入 Nginx 目录：

```bash
scripts/release/daemon/publish-linux.sh --base-url https://example.com --dry-run
```

## Nginx 静态文件要求

Daemon 安装脚本固定通过服务域名下的 `/daemon` 路径提供。

如果服务根地址是：

```text
https://example.com
```

那么 daemon 下载根地址就是：

```text
https://example.com/daemon
```

Nginx 对应的静态目录里必须有这些文件：

```text
<daemon-static-root>/
  install.sh
  releases/
    manifest.json
    <version>/
      awiki-deamon-linux-amd64.tar.gz
      checksums.txt
```

发布脚本默认写入的本机目录是：

```text
/var/www/awiki-web/daemon
```

如果服务器使用了其他静态目录，用 `AWIKI_DAEMON_NGINX_DIR` 覆盖：

```bash
AWIKI_DAEMON_NGINX_DIR=/srv/www/example/daemon \
  scripts/release/daemon/publish-linux.sh --base-url https://example.com
```

Nginx 虚拟主机必须把 `/daemon/` 路由到同一个目录。示例配置：

```nginx
server {
    server_name example.com;

    location ^~ /daemon/ {
        alias /var/www/awiki-web/daemon/;
        try_files $uri =404;
        default_type application/octet-stream;
    }
}
```

如果静态目录不是 `/var/www/awiki-web/daemon`，同步修改 `alias`，并在执行发布
脚本时设置相同的 `AWIKI_DAEMON_NGINX_DIR`。URL 路径仍保持 `/daemon/`。

## `--base-url` 的含义

`--base-url` 是目标环境的后端服务根地址，会写入生成后的安装脚本。发布脚本
会按固定规则推导 daemon 下载根地址：

```text
<base-url>/daemon
```

例如执行：

```bash
scripts/release/daemon/publish-linux.sh --base-url https://example.com
```

生成的安装脚本会使用：

```text
BASE_URL=https://example.com
DOWNLOAD_BASE_URL=https://example.com/daemon
```

不要把具体环境的真实域名写死在仓库里。不同环境发布时，通过 `--base-url`
传入对应域名。

## 发布后的验证

发布完成后至少验证这三个 URL：

```bash
curl -fsSL https://example.com/daemon/releases/manifest.json
curl -fsSIL https://example.com/daemon/install.sh
curl -fsSIL https://example.com/daemon/releases/<version>/awiki-deamon-linux-amd64.tar.gz
```

`manifest.json` 必须满足：

- `latest` 等于本次发布版本。
- `packages` 中存在 `linux/amd64` 包。
- `linux/amd64` 包的 `sha256` 是 64 位十六进制字符串。
- `linux/amd64` 包的 URL 指向本次发布版本目录。

## 内部 helper 职责

`_build-artifact.sh` 负责构建：

```text
awiki-deamon-linux-amd64.tar.gz
```

`_stage-downloads.sh` 负责生成最终下载目录：

```text
install.sh
releases/manifest.json
releases/<version>/...
```

`_generate-manifest.js` 负责扫描 staged 包并写入：

```text
releases/manifest.json
```

`_install.sh.template` 是发布产物 `install.sh` 的源模板。`_stage-downloads.sh`
会把模板里的 base URL 和 download base URL 占位符替换成发布参数，再写入最终
下载目录。

这些 helper 只用于发布流程排查。正常发布始终使用
`scripts/release/daemon/publish-linux.sh`。
