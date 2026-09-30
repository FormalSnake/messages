{
  description = "Messages: an iMessage client for Linux, backed by BlueBubbles";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      linuxSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = nixpkgs.lib.genAttrs linuxSystems (system: rec {
        messages = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { };
        default = messages;
      });

      overlays.default = final: _: { messages = final.callPackage ./nix/package.nix { }; };

      homeModules.default = import ./nix/hm-module.nix self;
      homeManagerModules.default = self.homeModules.default;

      devShells = forAll (pkgs:
        let
          # gpui-pre links libxkbcommon and freetype at build time and dlopens
          # wayland, vulkan, fontconfig and X11 at runtime.
          linuxLibs = with pkgs; [
            libxkbcommon
            wayland
            wayland-protocols
            vulkan-loader
            fontconfig.lib
            freetype
            libxcb
            libx11
            libxcursor
            libxi
            libxrandr
            libglvnd
            alsa-lib
          ];
        in
        {
          default = pkgs.mkShell {
            packages = [ pkgs.bun ] ++ pkgs.lib.optionals pkgs.stdenv.hostPlatform.isLinux ([ pkgs.cargo pkgs.rustc pkgs.fontconfig pkgs.fontconfig.dev pkgs.grim pkgs.wl-clipboard pkgs.libnotify pkgs.pkg-config ] ++ linuxLibs);
            # `cargo build -p messages` needs pkg-config to find libxkbcommon's and
            # freetype's headers; mkShell's setup hooks pick those up from the
            # packages above once pkg-config is present. The binary is built
            # outside the Nix sandbox, so the dlopened libraries go on
            # LD_LIBRARY_PATH for both linking and running.
            shellHook = pkgs.lib.optionalString pkgs.stdenv.hostPlatform.isLinux ''
              export LD_LIBRARY_PATH=/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath linuxLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
            '';
          };
        });
    };
}
