#!/usr/bin/env sh
set -eu

repository="${GROKEX_REPOSITORY:-Harness-X-Harness/codex}"
version="${GROKEX_VERSION:-0.148.0-alpha.5}"
tag="grokex-v${version}"

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *)
    echo "Grokex does not provide an archive for this operating system." >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  arm64|aarch64) arch="aarch64" ;;
  x86_64|amd64) arch="x86_64" ;;
  *)
    echo "Grokex does not provide an archive for this CPU architecture." >&2
    exit 1
    ;;
esac

target="${arch}-${os}"
asset="grokex-${target}.tar.gz"
release_url="https://github.com/${repository}/releases/download/${tag}"
install_root="${GROKEX_INSTALL_ROOT:-${HOME}/.local/share/grokex}"
bin_dir="${GROKEX_BIN_DIR:-${HOME}/.local/bin}"
codex_home="${GROKEX_CODEX_HOME:-${HOME}/.codex-grok}"

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf -- "$tmp_dir"
}
trap cleanup EXIT HUP INT TERM

curl -fsSL "${release_url}/${asset}" -o "${tmp_dir}/${asset}"
curl -fsSL "${release_url}/SHA256SUMS" -o "${tmp_dir}/SHA256SUMS"

expected="$(awk -v name="$asset" '$2 == name { print $1 }' "${tmp_dir}/SHA256SUMS")"
if [ -z "$expected" ]; then
  echo "SHA256SUMS does not contain ${asset}." >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${tmp_dir}/${asset}" | awk '{ print $1 }')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${tmp_dir}/${asset}" | awk '{ print $1 }')"
else
  echo "A SHA-256 utility is required." >&2
  exit 1
fi

if [ "$actual" != "$expected" ]; then
  echo "Checksum verification failed for ${asset}." >&2
  exit 1
fi

tar -xzf "${tmp_dir}/${asset}" -C "$tmp_dir"
package_root="${tmp_dir}/grokex-${target}"
if [ ! -x "${package_root}/bin/codex" ] || [ ! -x "${package_root}/bin/codex-code-mode-host" ]; then
  echo "The Grokex archive is incomplete." >&2
  exit 1
fi

version_root="${install_root}/versions/${version}"
current_root="${install_root}/current"
mkdir -p "${install_root}/versions" "$bin_dir" "$codex_home"
chmod 700 "$codex_home"
rm -rf -- "$version_root"
mkdir -p "$version_root"
cp -R "${package_root}/." "$version_root/"
ln -sfn "$version_root" "$current_root"

cat > "${bin_dir}/grokex" <<EOF
#!/usr/bin/env sh
CODEX_HOME="\${CODEX_HOME:-\$HOME/.codex-grok}"
export CODEX_HOME
exec "${current_root}/bin/codex" "\$@"
EOF
chmod 755 "${bin_dir}/grokex"

if [ ! -e "${codex_home}/config.toml" ]; then
  cp "${version_root}/config.toml.example" "${codex_home}/config.toml"
  chmod 600 "${codex_home}/config.toml"
fi

echo "Installed Grokex ${version} as ${bin_dir}/grokex"
echo "Ensure ${bin_dir} is in PATH, then run 'grokex login' for ChatGPT."
echo "Set GROK_API_KEY to use Grok, then run: grokex"
