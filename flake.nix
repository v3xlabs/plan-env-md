{
  description = "devshell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };

      rustToolchain = pkgs.rust-bin.stable.latest.default.override {
        extensions = [
          "rust-src"
          "llvm-tools"
        ];
      };

      rustfmtNightly = pkgs.rust-bin.nightly.latest.rustfmt;
    in {
      packages.plan-env-md-mcp = pkgs.rustPlatform.buildRustPackage {
        pname = "plan-env-md-mcp";
        version = "0.1.0";
        src = ./mcp;
        cargoLock.lockFile = ./mcp/Cargo.lock;
        SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        nativeBuildInputs = [pkgs.makeWrapper];
        postFixup = ''
          wrapProgram "$out/bin/plan-env-md-mcp" \
            --set SSL_CERT_FILE "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        '';
      };

      apps.plan-env-md-mcp = {
        type = "app";
        program = "${self.packages.${system}.plan-env-md-mcp}/bin/plan-env-md-mcp";
      };

      devShells = {
        default = pkgs.mkShell {
          packages = with pkgs; [
            rustfmtNightly
            rustToolchain
            rust-analyzer
            bacon
            just
            sqlx-cli

            nodejs_24
            pnpm_11
          ];

          shellHook = ''
            just
          '';
        };
      };
    });
}
