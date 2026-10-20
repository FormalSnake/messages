{
  description = "Messages: an iMessage client for Linux, backed by BlueBubbles";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.nativedesktop = {
    url = "github:FormalSnake/NativeDesktop";
    inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, nativedesktop }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      linuxSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = nixpkgs.lib.genAttrs linuxSystems (system: rec {
        messages = nixpkgs.legacyPackages.${system}.callPackage ./nix/package.nix { ndSrc = nativedesktop; };
        default = messages;
      });

      overlays.default = final: _: { messages = final.callPackage ./nix/package.nix { ndSrc = nativedesktop; }; };

      homeModules.default = import ./nix/hm-module.nix { inherit self nativedesktop; };
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
            # The viewer's video() loads GStreamer with dlopen; libgstapp is in plugins-base.
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
          ];
          gstPlugins = pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
            with pkgs.gst_all_1;
            [
              gstreamer
              gst-plugins-base
              gst-plugins-good
              gst-plugins-bad
              gst-libav
            ]
          );
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
              export GST_PLUGIN_SYSTEM_PATH_1_0=${gstPlugins}
            '';
          };
        });
    };
}
