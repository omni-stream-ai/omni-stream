{
  description = "OmniStream client development environment";

  inputs.nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";

  outputs = { nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    in {
      devShells = nixpkgs.lib.genAttrs systems (system:
        let pkgs = import nixpkgs { inherit system; };
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [ nodejs_24 pnpm rustup pkg-config ]
              ++ lib.optionals stdenv.hostPlatform.isLinux [
                webkitgtk_4_1 gtk3 glib libsoup_3 librsvg openssl dbus patchelf
              ];

            shellHook = ''
              if [[ -z "''${JAVA_HOME:-}" ]] && command -v javac >/dev/null 2>&1; then
                export JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v javac)")")")"
              fi
              export ANDROID_HOME="''${ANDROID_HOME:-''${ANDROID_SDK_ROOT:-}}"
              if [[ -n "$ANDROID_HOME" ]]; then
                export ANDROID_SDK_ROOT="$ANDROID_HOME"
                if [[ -z "''${NDK_HOME:-}" && -d "$ANDROID_HOME/ndk" ]]; then
                  export NDK_HOME="$(find -L "$ANDROID_HOME/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
                fi
                if [[ ! -d "''${NDK_HOME:-}/toolchains/llvm" ]]; then
                  echo 'Android NDK not found; set NDK_HOME before running Android commands.' >&2
                fi
                export PATH="$ANDROID_HOME/platform-tools:$PATH"
              fi
            '';
          };
        });
    };
}
