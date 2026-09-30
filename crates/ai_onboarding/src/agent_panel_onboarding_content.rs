use std::sync::Arc;

use gpui::{IntoElement, ParentElement};
use ui::{Tooltip, prelude::*};

use crate::{AgentPanelOnboardingCard, ApiKeysWithoutProviders};

pub struct AgentPanelOnboarding {
    on_dismiss: Arc<dyn Fn(&mut Window, &mut App)>,
}

impl AgentPanelOnboarding {
    pub fn new(on_dismiss: impl Fn(&mut Window, &mut App) + 'static, _cx: &mut Context<Self>) -> Self {
        Self {
            on_dismiss: Arc::new(on_dismiss),
        }
    }
}

impl Render for AgentPanelOnboarding {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        AgentPanelOnboardingCard::new().child(
            div()
                .relative()
                .child(ApiKeysWithoutProviders::new())
                .child(
                    h_flex().absolute().top_0().right_0().child(
                        IconButton::new("dismiss_onboarding", IconName::Close)
                            .icon_size(IconSize::Small)
                            .tooltip(Tooltip::text("Dismiss"))
                            .on_click({
                                let callback = self.on_dismiss.clone();
                                move |_, window, cx| {
                                    telemetry::event!(
                                        "Banner Dismissed",
                                        source = "AI Onboarding"
                                    );
                                    callback(window, cx)
                                }
                            }),
                    ),
                ),
        )
    }
}
