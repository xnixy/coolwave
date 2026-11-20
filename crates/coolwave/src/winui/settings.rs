//! The Settings page: how Coolwave behaves when closed and at sign-in.

use super::window::{MainWindow, Message};
use coolwave_ui::*;
use windows_reactor::*;

impl MainWindow {
    pub(super) fn settings_page(&self, context: &ViewContext<Self>) -> View {
        let run_in_background = self.core().settings.run_in_background;
        page_frame(StackPanel::new().spacing(22.0).max_width(1000.0).children((
            page_header("Settings", "How Coolwave runs in the background."),
            section(
                "Background",
                vec![
                    row(
                        "Keep running when closed",
                        "Effects, fan curves and the pump screen carry on from the tray.",
                        switch(run_in_background, "Keep running when closed", context.callback(Message::RunInBackground)),
                    ),
                    row(
                        "Start at sign-in",
                        "Coolwave starts in the tray when you sign in.",
                        switch(self.start_at_sign_in, "Start at sign-in", context.callback(Message::StartAtSignIn)),
                    ),
                ],
            ),
            section(
                "Coolwave",
                vec![row(
                    "Quit Coolwave",
                    "Stops the effects and leaves the tray.",
                    Button::new().content("Quit").automation_name("Quit Coolwave").on_click(context.message(Message::Quit)),
                )],
            ),
        )))
    }
}
