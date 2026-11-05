//! One module per screen. Each is a view built fresh when navigated to, which
//! fetches whatever it needs on creation.

use gpui_kit::{AnyView, AppContext as _, Context, Entity, Window, assets::IconName};

use crate::{app::Shell, model::AppModel};

mod allocation;
mod briefing;
mod documents;
mod health;
mod holdings;
mod overview;
mod performance;
mod policy;
mod rebalance;
mod review;
mod settings;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Overview,
    Holdings,
    Allocation,
    Performance,
    Review,
    Rebalance,
    Policy,
    Documents,
    Briefing,
    Health,
    Settings,
}

impl View {
    pub const PORTFOLIO: [View; 4] = [
        View::Overview,
        View::Holdings,
        View::Allocation,
        View::Performance,
    ];
    pub const PLAN: [View; 4] = [View::Review, View::Rebalance, View::Policy, View::Documents];
    pub const INSIGHTS: [View; 2] = [View::Briefing, View::Health];

    pub fn id(self) -> &'static str {
        match self {
            View::Overview => "nav-overview",
            View::Holdings => "nav-holdings",
            View::Allocation => "nav-allocation",
            View::Performance => "nav-performance",
            View::Review => "nav-review",
            View::Rebalance => "nav-rebalance",
            View::Policy => "nav-policy",
            View::Documents => "nav-documents",
            View::Briefing => "nav-briefing",
            View::Health => "nav-health",
            View::Settings => "nav-settings",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Holdings => "Holdings",
            View::Allocation => "Allocation",
            View::Performance => "Performance",
            View::Review => "Review",
            View::Rebalance => "Rebalance",
            View::Policy => "Policy",
            View::Documents => "Documents",
            View::Briefing => "Briefing",
            View::Health => "Data Health",
            View::Settings => "Settings",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            View::Overview => IconName::LayoutDashboard,
            View::Holdings => IconName::ChartColumn,
            View::Allocation => IconName::ChartPie,
            View::Performance => IconName::TrendingUp,
            View::Review => IconName::Stethoscope,
            View::Rebalance => IconName::Scale,
            View::Policy => IconName::Shield,
            View::Documents => IconName::FileText,
            View::Briefing => IconName::ClipboardList,
            View::Health => IconName::HeartPulse,
            View::Settings => IconName::Settings,
        }
    }

    pub fn needs_workspace(self) -> bool {
        Self::PLAN.contains(&self)
    }
}

pub fn build(
    view: View,
    model: &Entity<AppModel>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> AnyView {
    let model = model.clone();
    match view {
        View::Overview => cx.new(|cx| overview::Overview::new(model, cx)).into(),
        View::Holdings => cx
            .new(|cx| holdings::Holdings::new(model, window, cx))
            .into(),
        View::Allocation => cx.new(|cx| allocation::Allocation::new(model, cx)).into(),
        View::Performance => cx.new(|cx| performance::Performance::new(model, cx)).into(),
        View::Review => cx.new(|cx| review::ReviewPage::new(model, cx)).into(),
        View::Rebalance => cx.new(|cx| rebalance::Rebalance::new(model, cx)).into(),
        View::Policy => cx
            .new(|cx| policy::PolicyPage::new(model, window, cx))
            .into(),
        View::Documents => cx
            .new(|cx| documents::Documents::new(model, window, cx))
            .into(),
        View::Briefing => cx.new(|cx| briefing::Briefing::new(model, cx)).into(),
        View::Health => cx.new(|cx| health::Health::new(model, cx)).into(),
        View::Settings => cx
            .new(|cx| settings::Settings::new(model, window, cx))
            .into(),
    }
}
