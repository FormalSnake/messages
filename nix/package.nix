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
  gst_all_1,
  libnotify,
  xdg-utils,
  # NativeDesktop's source tree. The nd crates are path dependencies on the sibling checkout
  # `../nativedesktop-wt-msg-gaps`, so the build puts this tree at that path.
  ndSrc,
}:

let
  cargoToml = lib.importTOML ../crates/desktop/Cargo.toml;
  appId = "dev.formalsnake.messages";
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

  postUnpack = ''
    cp -r ${ndSrc} nativedesktop-wt-msg-gaps
    chmod -R u+w nativedesktop-wt-msg-gaps
  '';

  cargoLock = {
    lockFile = ../Cargo.lock;
    # nd-terminal's, not built on Linux but still in the lock.
    outputHashes."libghostty-vt-0.2.1" = "sha256-XepsHhhauHZnaz8ZpWNaas/7sI/22VoH7b3+0dP4NJk=";
  };
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
        # The viewer plays videos through GStreamer, which nd dlopens.
        gst_all_1.gstreamer
        gst_all_1.gst-plugins-base
      ]
    }
    wrapProgram $out/bin/messages --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : ${
      lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (
        with gst_all_1;
        [
          gstreamer
          gst-plugins-base
          gst-plugins-good
          gst-plugins-bad
          gst-libav
        ]
      )
    } --suffix PATH : ${
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
