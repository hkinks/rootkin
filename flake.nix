{
  description = "Rootkin: a private, offline family tree editor";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      runtimeLibs = pkgs: with pkgs; [ libGL vulkan-loader libxkbcommon wayland libx11 libxcursor libxi libxrandr ];
    in
    {
      packages = forAll (pkgs: rec {
        rootkin = pkgs.rustPlatform.buildRustPackage {
          pname = "rootkin";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          # Only the app; rootkin-cli is a developer tool (`cargo run -p rootkin-cli`)
          cargoBuildFlags = [ "-p" "rootkin" ];
          # Tests run in CI and in `nix flake check` (checks.tests), not on every install
          doCheck = false;
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postInstall = ''
            install -Dm644 assets/rootkin.desktop $out/share/applications/rootkin.desktop
            install -Dm644 assets/rootkin.svg $out/share/icons/hicolor/scalable/apps/rootkin.svg
            install -Dm644 assets/rootkin.png $out/share/icons/hicolor/256x256/apps/rootkin.png
            wrapProgram $out/bin/rootkin --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath (runtimeLibs pkgs)}
          '';
          meta = {
            description = "Private, offline family tree editor compatible with .familyx files";
            homepage = "https://github.com/hkinks/rootkin";
            license = with pkgs.lib.licenses; [ unlicense mit ];
            mainProgram = "rootkin";
          };
        };
        default = rootkin;
      });

      # `nix flake check`: the whole test suite
      checks = forAll (pkgs: {
        tests = self.packages.${pkgs.stdenv.hostPlatform.system}.rootkin.overrideAttrs {
          doCheck = true;
          cargoTestFlags = [ "--workspace" ];
        };
      });

      devShells = forAll (pkgs: {
        default = import ./shell.nix { inherit pkgs; };
      });
    };
}
