//! Launching the real app for tier 3. On Linux it runs in a private headless Hyprland started for
//! the test, so a run needs no desktop session and never touches one.

use nd_test::{AppHandle, LaunchOptions, launch_app};

/// The app and, on Linux, the compositor it draws into; dropping it ends both.
pub struct Running {
    // Field order is drop order: the app goes before its compositor.
    pub app: AppHandle,
    #[cfg(target_os = "linux")]
    compositor: ndshell::hyprland::Hyprland,
}

impl Running {
    /// The app's window as the compositor shows it: GPUI on Linux reads no frame back from a
    /// real window, so its screencopy (grim) takes it.
    #[cfg(target_os = "linux")]
    #[allow(dead_code)]
    pub fn grab(&self, out: &std::path::Path) {
        let client = self.compositor.wait_for_client(self.app.pid(), std::time::Duration::from_secs(10)).unwrap();
        let geometry = format!("{},{} {}x{}", client.at.0, client.at.1, client.size.0, client.size.1);
        let status = self.compositor.command("grim").arg("-g").arg(&geometry).arg(out).status().unwrap();
        assert!(status.success(), "grim -g {geometry} failed");
    }
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
        Running { app: launch_app(options).unwrap(), compositor }
    }
    #[cfg(not(target_os = "linux"))]
    Running { app: launch_app(options).unwrap() }
}
