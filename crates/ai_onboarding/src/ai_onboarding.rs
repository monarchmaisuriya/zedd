mod agent_api_keys_onboarding;
mod agent_panel_onboarding_card;
mod agent_panel_onboarding_content;
mod edit_prediction_onboarding_content;

pub use agent_api_keys_onboarding::{ApiKeysWithProviders, ApiKeysWithoutProviders};
pub use agent_panel_onboarding_card::AgentPanelOnboardingCard;
pub use agent_panel_onboarding_content::AgentPanelOnboarding;
pub use edit_prediction_onboarding_content::EditPredictionOnboarding;

use std::sync::Arc;

use client::Client;
use gpui::{AnyElement, IntoElement, ParentElement, TaskExt};
use ui::{RegisterComponent, Tooltip, prelude::*};

#[derive(PartialEq)]
pub enum SignInStatus {
    SignedIn,
    SigningIn,
    SignedOut,
}

impl From<client::Status> for SignInStatus {
    fn from(status: client::Status) -> Self {
        if status.is_signing_in() {
            Self::SigningIn
        } else if status.is_signed_out() {
            Self::SignedOut
        } else {
            Self::SignedIn
        }
    }
}

/// Offers Zed's built-in edit prediction model, replacing sign-in-required
/// language with a direct call to action once signed in.
#[derive(RegisterComponent, IntoElement)]
pub struct ZedAiOnboarding {
    pub sign_in_status: SignInStatus,
    pub continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
    pub sign_in: Arc<dyn Fn(&mut Window, &mut App)>,
    pub dismiss_onboarding: Option<Arc<dyn Fn(&mut Window, &mut App)>>,
}

impl ZedAiOnboarding {
    pub fn new(
        client: Arc<Client>,
        continue_with_zed_ai: Arc<dyn Fn(&mut Window, &mut App)>,
    ) -> Self {
        let status = *client.status().borrow();

        Self {
            sign_in_status: status.into(),
            continue_with_zed_ai,
            sign_in: Arc::new(move |_window, cx| {
                cx.spawn({
                    let client = client.clone();
                    async move |cx| client.sign_in_with_optional_connect(true, cx).await
                })
                .detach_and_log_err(cx);
            }),
            dismiss_onboarding: None,
        }
    }

    pub fn with_dismiss(
        mut self,
        dismiss_callback: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss_onboarding = Some(Arc::new(dismiss_callback));
        self
    }

    fn render_dismiss_button(&self) -> Option<AnyElement> {
        self.dismiss_onboarding.as_ref().map(|dismiss_callback| {
            let callback = dismiss_callback.clone();

            h_flex()
                .absolute()
                .top_0()
                .right_0()
                .child(
                    IconButton::new("dismiss_onboarding", IconName::Close)
                        .icon_size(IconSize::Small)
                        .tooltip(Tooltip::text("Dismiss"))
                        .on_click(move |_, window, cx| {
                            telemetry::event!("Banner Dismissed", source = "AI Onboarding",);
                            callback(window, cx)
                        }),
                )
                .into_any_element()
        })
    }

    fn render_sign_in_disclaimer(&self, _cx: &mut App) -> AnyElement {
        let signing_in = matches!(self.sign_in_status, SignInStatus::SigningIn);

        v_flex()
            .w_full()
            .relative()
            .gap_1()
            .child(Headline::new("Zed AI"))
            .child(
                Label::new("Sign in to use Zed's built-in edit prediction model.")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(
                Button::new("sign_in", "Sign In")
                    .disabled(signing_in)
                    .full_width()
                    .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                    .on_click({
                        let callback = self.sign_in.clone();
                        move |_, window, cx| callback(window, cx)
                    }),
            )
            .children(self.render_dismiss_button())
            .into_any_element()
    }

    fn render_signed_in_state(&self, _cx: &mut App) -> AnyElement {
        v_flex()
            .w_full()
            .relative()
            .gap_1()
            .child(Headline::new("Zed AI"))
            .child(
                Label::new("Zed's built-in edit prediction model is ready to use.")
                    .color(Color::Muted)
                    .mb_2(),
            )
            .child(
                Button::new("continue", "Use Zed AI")
                    .full_width()
                    .style(ButtonStyle::Tinted(ui::TintColor::Accent))
                    .on_click({
                        let callback = self.continue_with_zed_ai.clone();
                        move |_, window, cx| callback(window, cx)
                    }),
            )
            .children(self.render_dismiss_button())
            .into_any_element()
    }
}

impl RenderOnce for ZedAiOnboarding {
    fn render(self, _window: &mut ui::Window, cx: &mut App) -> impl IntoElement {
        if matches!(self.sign_in_status, SignInStatus::SignedIn) {
            self.render_signed_in_state(cx)
        } else {
            self.render_sign_in_disclaimer(cx)
        }
    }
}

impl Component for ZedAiOnboarding {
    fn scope() -> ComponentScope {
        ComponentScope::Onboarding
    }

    fn name() -> &'static str {
        "Zed AI Onboarding"
    }

    fn description() -> &'static str {
        "The onboarding surface offering Zed's built-in edit prediction model, shown before a user opts into it."
    }

    fn preview(_window: &mut Window, _cx: &mut App) -> AnyElement {
        fn onboarding(sign_in_status: SignInStatus) -> AnyElement {
            div()
                .w_full()
                .min_w_40()
                .max_w(px(1100.))
                .child(
                    AgentPanelOnboardingCard::new().child(
                        ZedAiOnboarding {
                            sign_in_status,
                            continue_with_zed_ai: Arc::new(|_, _| {}),
                            sign_in: Arc::new(|_, _| {}),
                            dismiss_onboarding: None,
                        }
                        .into_any_element(),
                    ),
                )
                .into_any_element()
        }

        v_flex()
            .min_w_0()
            .gap_4()
            .children(vec![
                single_example("Not Signed-in", onboarding(SignInStatus::SignedOut)),
                single_example("Signed-in", onboarding(SignInStatus::SignedIn)),
            ])
            .into_any_element()
    }
}
