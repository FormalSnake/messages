{
  description = "Messages for Linux: dev shell with the libraries the prebuilt gpuix renderer dlopens";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [ pkgs.bun ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.grim pkgs.wl-clipboard pkgs.libnotify ];
          # @gpuix/native ships a prebuilt .node that links libxkbcommon and dlopens
          # wayland, vulkan, fontconfig and X11 at runtime. Nix's bun does not read
          # NIX_LD_LIBRARY_PATH, so the libraries go on LD_LIBRARY_PATH instead.
          shellHook = pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export LD_LIBRARY_PATH=/run/opengl-driver/lib:${pkgs.lib.makeLibraryPath [
              pkgs.libxkbcommon
              pkgs.wayland
              pkgs.vulkan-loader
              pkgs.fontconfig.lib
              pkgs.freetype
              pkgs.libxcb
              pkgs.xorg.libX11
              pkgs.libglvnd
            ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
          '';
        };
      });
    };
}
