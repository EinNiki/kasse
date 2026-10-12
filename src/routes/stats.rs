use axum::extract::{Query, State};
use axum::response::Html;
use axum::Extension;
use chrono::{Duration, NaiveDate};
use serde::Deserialize;

use crate::auth::CurrentUser;
use crate::db::{self, stats::CategoryStat, stats::ProductStat};
use crate::error::AppError;
use crate::money::format_eur;
use crate::routes::{non_empty, page_ctx, render};
use crate::templates::{PeriodLink, SaleRow, StatRow, StatsTemplate};
use crate::AppState;

#[derive(Deserialize)]
pub struct StatsQuery {
    von: Option<String>,
    bis: Option<String>,
    /// Selected shortcut: heute, gestern, woche, monat
    zeitraum: Option<String>,
}

fn parse_date(s: Option<String>) -> Option<NaiveDate> {
    non_empty(s).and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

fn de_date(d: NaiveDate) -> String {
    d.format("%d.%m.%Y").to_string()
}

/// Share in percent with one decimal and German comma, e.g. "12,3 %".
fn share(part: i64, total: i64) -> String {
    if total <= 0 {
        return "–".to_string();
    }
    let tenths = part.saturating_mul(1000) / total;
    format!("{},{} %", tenths / 10, tenths % 10)
}

fn product_rows(stats: &[&ProductStat]) -> Vec<StatRow> {
    stats
        .iter()
        .take(10)
        .map(|p| StatRow {
            name: p.name.clone(),
            quantity: p.quantity,
            revenue: format_eur(p.revenue_cents),
            share: String::new(),
        })
        .collect()
}

fn category_rows(stats: &[CategoryStat], total: i64) -> Vec<StatRow> {
    stats
        .iter()
        .map(|c| StatRow {
            name: c.name.clone(),
            quantity: c.quantity,
            revenue: format_eur(c.revenue_cents),
            share: share(c.revenue_cents, total),
        })
        .collect()
}

pub async fn stats(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<StatsQuery>,
) -> Result<Html<String>, AppError> {
    let pool = &state.pool;
    let today = db::stats::today(pool).await?;
    let (week_start, month_start) = db::stats::week_and_month_start(pool).await?;

    let yesterday = today - Duration::days(1);
    // (url key, label, from, to)
    let shortcuts = [
        ("heute", "Heute", today, today),
        ("gestern", "Gestern", yesterday, yesterday),
        ("woche", "Woche", week_start, today),
        ("monat", "Monat", month_start, today),
    ];
    // A clicked shortcut wins; its key is kept in the URL, so it stays highlighted even when two
    // shortcuts describe the same days (on a Monday "Heute" and "Woche" do).
    // Otherwise the date fields decide. If "from" is after "to" the range becomes that one day.
    let picked =
        non_empty(query.zeitraum.clone()).and_then(|k| shortcuts.iter().position(|s| s.0 == k));
    let has_dates = query.von.is_some() || query.bis.is_some();
    let active = picked.or(if has_dates { None } else { Some(0) });
    let (from, to) = match active {
        Some(i) => (shortcuts[i].2, shortcuts[i].3),
        None => {
            let from = parse_date(query.von.clone()).unwrap_or(today);
            let to = parse_date(query.bis.clone()).unwrap_or(today);
            (from, to.max(from))
        }
    };
    let links = shortcuts
        .iter()
        .enumerate()
        .map(|(i, &(key, label, _, _))| PeriodLink {
            label,
            href: format!("/statistik?zeitraum={key}"),
            active: Some(i) == active,
        })
        .collect();

    let day = db::stats::totals(pool, today, today).await?;
    let week = db::stats::totals(pool, week_start, today).await?;
    let month = db::stats::totals(pool, month_start, today).await?;
    let range = db::stats::totals(pool, from, to).await?;

    let products = db::stats::product_stats(pool, from, to).await?;
    let mut by_quantity: Vec<&ProductStat> = products.iter().collect();
    by_quantity.sort_by(|a, b| {
        b.quantity
            .cmp(&a.quantity)
            .then(b.revenue_cents.cmp(&a.revenue_cents))
            .then(a.name.cmp(&b.name))
    });
    let mut by_revenue: Vec<&ProductStat> = products.iter().collect();
    by_revenue.sort_by(|a, b| {
        b.revenue_cents
            .cmp(&a.revenue_cents)
            .then(b.quantity.cmp(&a.quantity))
            .then(a.name.cmp(&b.name))
    });

    let categories = db::stats::category_stats(pool, from, to).await?;
    let recent = db::stats::recent_sales(pool, from, to, 25).await?;

    let average = if range.sales_count > 0 {
        range.revenue_cents / range.sales_count
    } else {
        0
    };
    render(StatsTemplate {
        ctx: page_ctx(&user),
        title: "Statistik",
        from: from.to_string(),
        to: to.to_string(),
        period_label: if from == to {
            de_date(from)
        } else {
            format!("{} – {}", de_date(from), de_date(to))
        },
        links,
        today: format_eur(day.revenue_cents),
        week: format_eur(week.revenue_cents),
        month: format_eur(month.revenue_cents),
        revenue: format_eur(range.revenue_cents),
        sales_count: range.sales_count,
        average: format_eur(average),
        top_quantity: product_rows(&by_quantity),
        top_revenue: product_rows(&by_revenue),
        categories: category_rows(&categories, range.revenue_cents),
        sales: recent
            .into_iter()
            .map(|s| SaleRow {
                time: s.time,
                user: s.username,
                items: s.items,
                total: format_eur(s.total_cents),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::share;

    #[test]
    fn share_formats_percent() {
        assert_eq!(share(1, 3), "33,3 %");
        assert_eq!(share(5, 5), "100,0 %");
        assert_eq!(share(0, 5), "0,0 %");
        assert_eq!(share(1, 0), "–");
    }
}
