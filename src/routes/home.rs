use axum::extract::State;
use axum::response::Html;
use axum::Extension;
use rand::seq::IndexedRandom;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::money::format_eur;
use crate::routes::{page_ctx, render};
use crate::templates::HomeTemplate;
use crate::{db, AppState};

/// Playful headlines: (text before, emphasised word, text after). One is picked per page load.
pub const HEADLINES: &[(&str, &str, &str)] = &[
    ("Was steht ", "jetzt", " an?"),
    ("Von ", "Umsatz", " allein lebt man nicht."),
    ("Die Kasse klingelt. ", "Wer", " noch?"),
    ("Heute schon ", "Durst", " verkauft?"),
    ("Kein Bier ist auch ", "keine", " Lösung."),
    ("Prost, ", "Chef", "! Was darf's sein?"),
    ("Ein Tag ohne ", "Umsatz", " ist wie Bier ohne Schaum."),
    ("Bestand prüfen oder ", "Daumen", " drücken?"),
    ("Zählen kann jeder, ", "buchen", " können wir."),
    ("Noch ", "Kleingeld", " oder schon Feierabend?"),
];

pub async fn home(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Html<String>, AppError> {
    let summary = db::stats::home_summary(&state.pool).await?;
    render(HomeTemplate {
        ctx: page_ctx(&user),
        title: "Start",
        is_admin: user.is_admin,
        headline: *HEADLINES.choose(&mut rand::rng()).unwrap_or(&HEADLINES[0]),
        revenue: format_eur(summary.revenue_cents),
        sales_count: summary.sales_count,
        product_count: summary.product_count,
        sold_out_count: summary.sold_out_count,
    })
}
