# The home-manager module, generated: NativeDesktop's shared module (package, autostart,
# environmentFile, theme.json) with `programs.messages.settings` typed from the settings struct
# in crates/desktop/src/settings.rs (nix/settings-options.nix, rewritten by
# `MESSAGES_WRITE_SCHEMA=1 cargo test -p messages settings`).
{ self, nativedesktop }:
nativedesktop.lib.mkHomeModule {
  pname = "messages";
  settingsOptions = import ./settings-options.nix;
  defaultPackage =
    system:
    self.packages.${system}.default
      or (throw "messages: no Nix package for ${system}, set programs.messages.package");
}
