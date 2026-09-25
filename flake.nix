{
  description = "Messages for Linux: dev shell with the libraries the prebuilt gpuix renderer dlopens";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAll (pkgs:
        let
          # gpui-pre (the Rust rewrite's `crates/desktop`, vendored from Zed) links
          # libxkbcommon and freetype at build time and dlopens wayland, vulkan and
          # X11 at runtime, on top of what @gpuix/native (the TS app) already dlopens.
          linuxLibs = with pkgs; [
            libxkbcommon
            wayland
            wayland-protocols
            vulkan-loader
            fontconfig.lib
            freetype
            libxcb
            xorg.libX11
            xorg.libXcursor
            xorg.libXi
            xorg.libXrandr
            libglvnd
          ];
        in
        {
          default = pkgs.mkShell {
            packages = [ pkgs.bun ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux ([ pkgs.cargo pkgs.rustc pkgs.fontconfig.dev pkgs.grim pkgs.wl-clipboard pkgs.libnotify pkgs.pkg-config ] ++ linuxLibs);
            # `cargo build -p messages` needs pkg-config to find libxkbcommon's and
            # freetype's headers; mkShell's setup hooks pick those up from the
            # packages above automatically once pkg-config is present.
            #
            # @gpuix/native ships a prebuilt .node that links libxkbcommon and dlopens
            # wayland, vulkan, fontconfig and X11 at runtime. Nix's bun does not read
            # NIX_LD_LIBRARY_PATH, so the libraries go on LD_LIBRARY_PATH instead, and
            # the Rust binary (built outside the Nix sandbox) needs
            # the same at both link and run time.
            shellHook = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
              export LD_LIBRARY_PATH=/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath linuxLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
            '';
          };
        });
    };
}
