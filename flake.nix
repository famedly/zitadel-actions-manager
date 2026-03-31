# SPDX-FileCopyrightText: 2026 Famedly GmbH
#
# SPDX-License-Identifier: Apache-2.0

# flake.nix template for Rust repositories.
# Copy this file to your repo root and adjust as needed.
#
# Uses crane + fenix for Rust builds. The engineering-standards rust module
# generates all checks (clippy, fmt, nextest, deny), packages, and dev shell.
{
  description = "REPLACE_WITH_REPO_DESCRIPTION";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    engineering-standards.url = "git+https://github.com/famedly/engineering-standards?ref=feat-refactoring";
    crane.url = "github:ipetkov/crane";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { flake-parts, ... }@inputs:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.engineering-standards.flakeModules.default ];

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      perSystem =
        {
          pkgs,
          lib,
          system,
          ...
        }:
        let
          fenixPkgs = inputs.fenix.packages.${system};

          # Combined toolchain: stable Rust + nightly rustfmt.
          # Nightly rustfmt is required because rustfmt.toml uses unstable
          # options (imports_granularity, group_imports, wrap_comments, …).
          toolchain = fenixPkgs.combine [
            fenixPkgs.stable.cargo
            fenixPkgs.stable.clippy
            fenixPkgs.stable.rust-src
            fenixPkgs.stable.rustc
            fenixPkgs.latest.rustfmt
          ];
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchain;
        in
        {
          famedly.standards = {
            rules.enable = false;
            linting = {
              enable = true;
              rust = true;
            };
            preCommitHooks = {
              enable = true;
              rustHooks.enable = true;
            };
            infrastructure = {
              editorconfig = true;
              dependabot = true;
              dependabotRust = true;
              dependabotDocker = true;
            };
            devShell.enable = true;

            rust = {
              enable = true;
              inherit craneLib;
              src = ./.;
              # checks.clippy.useTestSrc = true;  # uncomment if tests use include_str!/include_bytes!
              docker.enable = true;
            };
          };

          famedly.github.workflows = {
            ci = {
              enable = true;
              armRunners = false;
            };
            "general-checks".enable = true;
            "ai-review".enable = false;
            "rust-ci".enable = true;
            publish-crate = {
              enable = true;
            };
            docker-backend = {
              enable = true;
              targets = "zitadel-actions-manager";
            };
            "fast-forward".enable = true;
          };
        };
    };
}
