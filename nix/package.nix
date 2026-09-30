{
  lib,
  rustPlatform,
  pkg-config,
  makeBinaryWrapper,
  alsa-lib,
  fontconfig,
  freetype,
  libxkbcommon,
  wayland,
  vulkan-loader,
  libGL,
  libx11,
  libxcb,
  libxcursor,
  libxi,
  libxrandr,
  ffmpeg,
  libnotify,
  xdg-utils,
}:

let
  cargoToml = lib.importTOML ../crates/desktop/Cargo.toml;
  appId = "es.canarycoders.messages";
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "messages";
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../packaging
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;
  cargoBuildFlags = [ "--package=messages" ];

  nativeBuildInputs = [
    pkg-config
    makeBinaryWrapper
  ];

  buildInputs = [
    alsa-lib
    fontconfig
    freetype
    libxkbcommon
    wayland
    libxcb
  ];

  env = {
    CARGO_PROFILE_RELEASE_DEBUG = "false";
    MESSAGES_ICON_PATH = "${placeholder "out"}/share/icons/hicolor/512x512/apps/${appId}.png";
  };

  # The GPUI tests need a window server and the core tests shell out to ffmpeg.
  doCheck = false;

  postInstall = ''
    install -Dm644 packaging/linux/${appId}.png -t $out/share/icons/hicolor/512x512/apps
    install -Dm644 packaging/linux/${appId}.desktop -t $out/share/applications
  '';

  # GPUI dlopens the windowing and GPU libraries at runtime.
  postFixup = ''
    patchelf $out/bin/messages --add-rpath ${
      lib.makeLibraryPath [
        vulkan-loader
        wayland
        libxkbcommon
        libGL
        libx11
        libxcb
        libxcursor
        libxi
        libxrandr
      ]
    }
    wrapProgram $out/bin/messages --suffix PATH : ${
      lib.makeBinPath [
        ffmpeg
        fontconfig.bin
        libnotify
        xdg-utils
      ]
    }
  '';

  meta = {
    description = "iMessage client for Linux, backed by a BlueBubbles server on a Mac";
    homepage = "https://github.com/FormalSnake/messages";
    license = lib.licenses.mit;
    mainProgram = "messages";
    platforms = lib.platforms.linux;
  };
})
