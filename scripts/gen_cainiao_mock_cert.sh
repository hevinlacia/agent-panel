#!/usr/bin/env bash
# 生成菜鸟打印 mock 的 wss 本地证书：自签 CA + localhost SAN 服务器证书。
#
# 产物（默认 ~/.local/share/agent-panel/certs/）：
#   ca.crt        自签 CA 证书（安装到浏览器/系统信任库）
#   ca.key        CA 私钥（仅本地保留，勿分发）
#   localhost.crt localhost 服务器证书（SAN: DNS:localhost, IP:127.0.0.1, IP:::1）
#   localhost.key localhost 服务器私钥（mock 进程读取，chmod 600）
#
# agent-panel 的 cainiao mock 检测到 localhost.crt + localhost.key 后，
# 自动在 wss://localhost:13529 启动 TLS 监听（https 页面的菜鸟组件入口）。
#
# 用法:
#   scripts/gen_cainiao_mock_cert.sh            # 已存在则跳过
#   scripts/gen_cainiao_mock_cert.sh --force    # 覆盖重新生成
#
# 安装信任（Chrome on Linux, 用户级、无需 sudo）:
#   certutil -d sql:"$HOME/.pki/nssdb" -A -t "C,," -n "agent-panel-cainiao-mock-ca" -i "$DIR/ca.crt"
set -euo pipefail

DIR="${CAINIAO_MOCK_CERT_DIR:-$HOME/.local/share/agent-panel/certs}"
FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

if [[ -f "$DIR/localhost.crt" && -f "$DIR/localhost.key" && "$FORCE" -eq 0 ]]; then
  echo "[skip] 证书已存在: $DIR （加 --force 重新生成）"
  exit 0
fi

mkdir -p "$DIR"

echo "[1/3] 生成本地开发 CA（10 年）..."
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$DIR/ca.key" -out "$DIR/ca.crt" \
  -subj "/CN=agent-panel cainiao mock dev CA" \
  -addext "basicConstraints=critical,CA:TRUE" \
  -addext "keyUsage=critical,keyCertSign,cRLSign"

echo "[2/3] 生成 localhost 服务器证书（SAN: localhost / 127.0.0.1 / ::1，5 年）..."
CONF="$(mktemp)"
cat > "$CONF" <<'EOF'
subjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1
extendedKeyUsage=serverAuth
EOF
openssl req -newkey rsa:2048 -nodes \
  -keyout "$DIR/localhost.key" -out "$DIR/localhost.csr" \
  -subj "/CN=localhost"
openssl x509 -req -in "$DIR/localhost.csr" \
  -CA "$DIR/ca.crt" -CAkey "$DIR/ca.key" -CAcreateserial \
  -days 1825 -extfile "$CONF" -out "$DIR/localhost.crt"
rm -f "$CONF" "$DIR/localhost.csr" "$DIR/ca.srl"

echo "[3/3] 收紧权限..."
chmod 600 "$DIR/localhost.key" "$DIR/ca.key"

echo "[ok] 证书已生成: $DIR"
echo "下一步：把 CA 装入 Chrome 信任库（用户级，无需 sudo）："
echo "  certutil -d sql:\"\$HOME/.pki/nssdb\" -A -t \"C,,\" -n \"agent-panel-cainiao-mock-ca\" -i \"$DIR/ca.crt\""
