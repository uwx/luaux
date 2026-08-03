{
  # Nix users install straight from the repository:
  #
  #   nix run github:luau-xml/luaux -- build src out
  #   nix profile install github:luau-xml/luaux
  #
  # The version comes from Cargo.toml, so it cannot drift from what the
  # release workflow verifies. Submission to nixpkgs proper is a separate,
  # manual step; this flake is what makes the project buildable there with a
  # `cargoHash` swap.
  description = "A compiler that turns .luaux — Luau with JSX syntax — into plain .luau";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
    in
    {
      packages = forAllSystems (pkgs: rec {
        default = luaux;

        luaux = pkgs.rustPlatform.buildRustPackage {
          pname = "luaux";
          inherit version;
          src = self;

          # The committed Cargo.lock is the source of truth, so no vendored
          # hash needs updating when dependencies change.
          cargoLock.lockFile = ./Cargo.lock;

          # Builds the CLI; the library comes along as its dependency. The
          # check phase still tests the whole workspace, matching CI. The
          # corpus and runtime suites skip themselves without their env vars,
          # exactly as they do in a plain `cargo test`.
          cargoBuildFlags = [ "-p" "luaux-cli" ];

          meta = with pkgs.lib; {
            description = "JSX syntax in Luau, compiled to Vide";
            homepage = "https://github.com/luau-xml/luaux";
            license = licenses.mit;
            mainProgram = "luaux";
          };
        };
      });

      # Everything a contributor needs, including the script that regenerates
      # the Roblox API tables.
      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer python3 ];
        };
      });
    };
}
