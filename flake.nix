{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nmattia/naersk/master";
  };

  outputs =
    {
      nixpkgs,
      utils,
      naersk,
      ...
    }:
    utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { };

        runtimeLibs = with pkgs; [
          libGL
          libxkbcommon
          wayland
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          alsa-lib
          # Needed for cargo-outdated
          openssl
        ];

        libPath = pkgs.lib.makeLibraryPath runtimeLibs;

        icon = ./static/sedentary.webp;
        name = "sedentary";
      in
      {
        packages.default = naersk-lib.buildPackage {
          src = ./.;
          doCheck = true;
          pname = "${name}";
          nativeBuildInputs = with pkgs; [
            makeWrapper
            pkg-config
            imagemagick
          ];
          buildInputs = runtimeLibs;
          postInstall = ''
            wrapProgram "$out/bin/sedentary" --prefix LD_LIBRARY_PATH : "${libPath}"
            for i in 16 24 48 64 96 128 256 512; do
              mkdir -p $out/share/icons/hicolor/''${i}x''${i}/apps
              convert -background none -resize ''${i}x''${i} ${icon} $out/share/icons/hicolor/''${i}x''${i}/apps/${name}.png
            done
          '';
        };

        devShells.default =
          pkgs.mkShell.override
            {
              stdenv = pkgs.stdenvAdapters.useMoldLinker pkgs.clangStdenv;
            }
            {
              nativeBuildInputs = [
                pkgs.pkg-config

                (pkgs.writeShellScriptBin "clippy" ''
                  cargo clippy --all-targets --all-features "$@"
                '')
                (pkgs.writeShellScriptBin "run-tests" ''
                  cargo test --all-targets --all-features "$@"
                '')
              ];

              buildInputs =
                with pkgs;
                [
                  cargo
                  rust-analyzer
                  rustPackages.clippy
                  rustc
                  rustfmt
                  tokei
                  lldb
                ]
                ++ runtimeLibs;
              RUST_SRC_PATH = pkgs.rustPlatform.rustLibSrc;
              LD_LIBRARY_PATH = libPath;
              shellHook = ''
                export PATH=$PATH:${pkgs.vscode-extensions.vadimcn.vscode-lldb}/share/vscode/extensions/vadimcn.vscode-lldb/adapter
              '';
            };
      }
    );
}
