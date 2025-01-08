{
  inputs = {
    naersk.url = "github:nmattia/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = {
    self,
    nixpkgs,
    utils,
    naersk,
    ...
  }:
    utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {inherit system;};
      fmtr = nixpkgs.legacyPackages.${system}.alejandra;
      naersk-lib = pkgs.callPackage naersk {};
      libPath = with pkgs;
        lib.makeLibraryPath [
          libGL
          libxkbcommon
          wayland
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          alsa-lib
        ];
      icon = pkgs.fetchurl {
        url = "https://raw.githubusercontent.com/busyboredom/sedentary/refs/heads/main/static/sedentary.webp";
        sha256 = "sha256-13RqUE7CFd08t0B5BbSM4InASaR1zc93/aM90QjiKrE=";
      };
      name = "sedentary";
    in {
      formatter = fmtr;
      defaultPackage = naersk-lib.buildPackage {
        src = ./.;
        doCheck = true;
        pname = "${name}";
        nativeBuildInputs = [
          pkgs.makeWrapper
          pkgs.pkg-config
          pkgs.imagemagick
        ];
        buildInputs = with pkgs; [
          xorg.libxcb
          alsa-lib
        ];
        postInstall = ''
          wrapProgram "$out/bin/sedentary" --prefix LD_LIBRARY_PATH : "${libPath}"
          for i in 16 24 48 64 96 128 256 512; do
            mkdir -p $out/share/icons/hicolor/''${i}x''${i}/apps
            convert -background none -resize ''${i}x''${i} ${icon} $out/share/icons/hicolor/''${i}x''${i}/apps/${name}.png
          done
        '';
      };

      defaultApp = utils.lib.mkApp {
        drv = self.defaultPackage."${system}";
      };

      devShell = with pkgs;
        mkShell {
          nativeBuildInputs = [
            pkgs.pkg-config
          ];
          buildInputs = [
            cargo
            cargo-insta
            pre-commit
            rust-analyzer
            rustPackages.clippy
            rustc
            rustfmt
            tokei

            xorg.libxcb
            alsa-lib
          ];
          RUST_SRC_PATH = rustPlatform.rustLibSrc;
          LD_LIBRARY_PATH = libPath;
          GIT_EXTERNAL_DIFF = "${difftastic}/bin/difft";
        };
    });
}
