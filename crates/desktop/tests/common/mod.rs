//! Launching the real app for tier 3. On Linux it runs in a private headless Hyprland started for
//! the test, so a run needs no desktop session and never touches one.

use nd_test::{AppHandle, LaunchOptions, launch_app};

/// The app and, on Linux, the compositor it draws into; dropping it ends both.
pub struct Running {
    // Field order is drop order: the app goes before its compositor.
    pub app: AppHandle,
    #[cfg(target_os = "linux")]
    _compositor: ndshell::hyprland::Hyprland,
}

impl std::ops::Deref for Running {
    type Target = AppHandle;
    fn deref(&self) -> &AppHandle {
        &self.app
    }
}

pub fn launch(options: LaunchOptions) -> Running {
    #[cfg(target_os = "linux")]
    {
        let compositor = ndshell::hyprland::Hyprland::start(ndshell::hyprland::Options {
            width: 1400,
            height: 900,
            ..Default::default()
        })
        .unwrap();
        let mut options = options;
        for (key, value) in compositor.client_env() {
            options = options.env(key, value);
        }
        Running { app: launch_app(options).unwrap(), _compositor: compositor }
    }
    #[cfg(not(target_os = "linux"))]
    Running { app: launch_app(options).unwrap() }
}
