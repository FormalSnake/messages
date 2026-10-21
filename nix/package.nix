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
  stdenv,
  libicns,
}:

let
  cargoToml = lib.importTOML ../crates/desktop/Cargo.toml;
  appId = "dev.formalsnake.messages";
  inherit (stdenv.hostPlatform) isDarwin isLinux;
  infoPlist = builtins.toFile "Info.plist" (lib.generators.toPlist { escape = true; } {
    CFBundleDevelopmentRegion = "en";
    CFBundleExecutable = "messages";
    CFBundleIdentifier = appId;
    CFBundleInfoDictionaryVersion = "6.0";
    CFBundleName = "Messages";
    CFBundleDisplayName = "Messages";
    CFBundlePackageType = "APPL";
    CFBundleShortVersionString = cargoToml.package.version;
    CFBundleVersion = cargoToml.package.version;
    CFBundleIconFile = "AppIcon";
    LSMinimumSystemVersion = "14.0";
    NSPrincipalClass = "NSApplication";
    NSHighResolutionCapable = true;
  });
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

  cargoLock = {
    lockFile = ../Cargo.lock;
    # NativeDesktop's crates come from git at the rev Cargo.lock pins.
    allowBuiltinFetchGit = true;
    # nd-terminal's, not built on Linux but still in the lock.
    outputHashes."libghostty-vt-0.2.1" = "sha256-XepsHhhauHZnaz8ZpWNaas/7sI/22VoH7b3+0dP4NJk=";
  };
  cargoBuildFlags = [
    "--package=messages"
  ]
  # nd's AppKit backend links Swift from Xcode, so the link and rustc's strip use the system
  # tools: Nix's cc wrapper brings its own SDK and Nix's strip corrupts proc-macro dylibs. The
  # hooks set the linker in the environment, which only `--config` outranks. This needs the Nix
  # sandbox off, the default on macOS.
  ++ lib.optional isDarwin ''--config=target.aarch64-apple-darwin.linker="/usr/bin/clang"'';

  nativeBuildInputs = [
    pkg-config
    makeBinaryWrapper
  ] ++ lib.optional isDarwin libicns;

  buildInputs = lib.optionals isLinux [
    alsa-lib
    fontconfig
    freetype
    libxkbcommon
    wayland
    libxcb
  ];

  env = {
    CARGO_PROFILE_RELEASE_DEBUG = "false";
  };

  preBuild = lib.optionalString isDarwin ''
    unset SDKROOT DEVELOPER_DIR
    mkdir -p "$TMPDIR/apple-bin"
    ln -s /usr/bin/strip /usr/bin/xcrun "$TMPDIR/apple-bin/"
    export PATH="$TMPDIR/apple-bin:$PATH"
  '';

  # The GPUI tests need a window server and the core tests shell out to ffmpeg.
  doCheck = false;

  postInstall = ''
    install -Dm644 packaging/linux/${appId}.png -t $out/share/icons/hicolor/512x512/apps
  '' + lib.optionalString isLinux ''
    install -Dm644 packaging/linux/${appId}.desktop -t $out/share/applications
  '' + lib.optionalString isDarwin ''
    app=$out/Applications/Messages.app/Contents
    install -Dm644 ${infoPlist} $app/Info.plist
    echo -n 'APPL????' > $app/PkgInfo
    mkdir -p $app/MacOS $app/Resources
    png2icns $app/Resources/AppIcon.icns packaging/linux/${appId}.png
    mv $out/bin/messages $app/MacOS/messages
    ln -s $app/MacOS/messages $out/bin/messages
  '';

  dontStrip = isDarwin;

  # GPUI dlopens the windowing and GPU libraries at runtime on Linux.
  postFixup = lib.optionalString isDarwin ''
    wrapProgram $out/Applications/Messages.app/Contents/MacOS/messages --suffix PATH : ${lib.makeBinPath [ ffmpeg ]}
  '' + lib.optionalString isLinux ''
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
    homepage = "https://github.com/FormalStudio/messages";
    license = lib.licenses.mit;
    mainProgram = "messages";
    platforms = lib.platforms.linux ++ [ "aarch64-darwin" ];
  };
})
