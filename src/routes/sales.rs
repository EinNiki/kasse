use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use serde::Deserialize;

use crate::auth::{verify_csrf, CurrentUser};
use crate::cart::{self, CartLine};
use crate::db;
use crate::db::sales::{cart_items, cart_total, CartItem, CheckoutError};
use crate::error::AppError;
use crate::money::{change_cents, format_eur, parse_eur};
use crate::routes::{non_empty, page_ctx, render, render_invalid, safe_back, urlencode};
use crate::templates::*;
use crate::AppState;

/// Cart with current prices (deleted products dropped) and the total.
async fn load_cart(state: &AppState, user: &CurrentUser) -> Result<(Vec<CartItem>, i64), AppError> {
    let lines = db::sessions::cart(&state.pool, &user.token_hash).await?;
    let items = cart_items(&state.pool, &lines).await?;
    let total =
        cart_total(&items).ok_or_else(|| AppError::BadRequest("Die Summe ist zu groß.".into()))?;
    Ok((items, total))
}

// ---------- Category selection ----------

pub async fn categories(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<Html<String>, AppError> {
    let rows = db::categories::list_with_counts(&state.pool).await?;
    let (items, _) = load_cart(&state, &user).await?;
    render(BookCategoriesTemplate {
        ctx: page_ctx(&user),
        title: "Buchen / Bestellen",
        categories: rows
            .into_iter()
            .map(|c| CategoryTile {
                id: c.id,
                name: c.name,
                product_count: c.product_count,
            })
            .collect(),
        cart_count: items.iter().map(|i| i.quantity).sum(),
    })
}

// ---------- Product tiles + cart ----------

#[derive(Deserialize)]
pub struct ShopQuery {
    q: Option<String>,
}

pub async fn shop(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(cat): Path<String>,
    Query(query): Query<ShopQuery>,
) -> Result<Html<String>, AppError> {
    let category: Option<i64> = if cat == "alle" {
        None
    } else {
        Some(
            cat.parse()
                .map_err(|_| AppError::BadRequest("Unbekannte Kategorie.".into()))?,
        )
    };
    let q = non_empty(query.q);
    let products = db::products::list(&state.pool, q.as_deref(), category).await?;
    let categories = db::categories::list(&state.pool).await?;
    let (items, total) = load_cart(&state, &user).await?;

    let mut chips = vec![Chip {
        href: "/buchen/alle".into(),
        name: "Alle".into(),
        active: category.is_none(),
    }];
    chips.extend(categories.into_iter().map(|c| Chip {
        href: format!("/buchen/{}", c.id),
        active: Some(c.id) == category,
        name: c.name,
    }));

    let in_cart = |id: i64| {
        items
            .iter()
            .find(|i| i.product_id == id)
            .map_or(0, |i| i.quantity)
    };
    let tiles = products
        .iter()
        .map(|p| ProductTile {
            id: p.id,
            name: p.name.clone(),
            price: format_eur(p.price_cents),
            stock: p.stock,
            disabled: p.stock <= in_cart(p.id),
            low: p.stock > 0 && p.stock <= 5,
        })
        .collect();

    let search_action = format!("/buchen/{cat}");
    let back = match &q {
        Some(q) => format!("{search_action}?q={}", urlencode(q)),
        None => search_action.clone(),
    };
    render(ShopTemplate {
        ctx: page_ctx(&user),
        title: "Buchen / Bestellen",
        search_action,
        q: q.unwrap_or_default(),
        chips,
        tiles,
        cart: items
            .iter()
            .map(|i| CartLineView {
                id: i.product_id,
                name: i.name.clone(),
                quantity: i.quantity,
                unit_price: format_eur(i.unit_price_cents),
                line_total: format_eur(i.unit_price_cents * i64::from(i.quantity)),
                can_plus: i.quantity < i.stock,
            })
            .collect(),
        total: format_eur(total),
        back,
        notice: None,
    })
}

#[derive(Deserialize)]
pub struct CartForm {
    csrf_token: String,
    back: Option<String>,
}

pub async fn cart_action(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path((product_id, action)): Path<(i64, String)>,
    Form(form): Form<CartForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let back = safe_back(form.back.as_deref().unwrap_or("/buchen"));
    match action.as_str() {
        "add" | "plus" => {
            // The stock limit is read first; the cart itself never holds more than is available.
            let stock = db::products::get(&state.pool, product_id)
                .await?
                .map_or(0, |p| p.stock);
            db::sessions::update_cart(&state.pool, &user.token_hash, |l| {
                cart::add(l, product_id, stock)
            })
            .await?;
        }
        "minus" => {
            db::sessions::update_cart(&state.pool, &user.token_hash, |l| {
                cart::decrement(l, product_id)
            })
            .await?;
        }
        "remove" => {
            db::sessions::update_cart(&state.pool, &user.token_hash, |l| {
                cart::remove(l, product_id)
            })
            .await?;
        }
        _ => return Err(AppError::BadRequest("Ungültige Aktion.".into())),
    }
    Ok(Redirect::to(&back).into_response())
}

// ---------- Payment ----------

fn change_view(total: i64, given: &str) -> ChangeView {
    match parse_eur(given) {
        None => ChangeView {
            state: "empty",
            amount: String::new(),
        },
        Some(paid) => match change_cents(total, paid) {
            Some(change) => ChangeView {
                state: "ok",
                amount: format_eur(change),
            },
            None => ChangeView {
                state: "short",
                amount: format_eur(total - paid),
            },
        },
    }
}

#[derive(Deserialize)]
pub struct GivenQuery {
    gegeben: Option<String>,
    /// Set by the quick-amount buttons: the input field is then rebuilt with the new value.
    k: Option<String>,
}

async fn pay_page(
    total: i64,
    given: String,
    quick_set: bool,
    error: Option<String>,
    user: &CurrentUser,
) -> Result<PayTemplate, AppError> {
    // Quick amounts: whole banknotes that cover the total, plus the exact amount.
    let mut quick: Vec<(String, String)> = [500, 1000, 2000, 5000, 10000]
        .into_iter()
        .filter(|&n| n >= total)
        .map(|n| {
            (
                format_eur(n).replace(",00", ""),
                format!("/buchen/abbuchen?k=1&gegeben={}", n / 100),
            )
        })
        .collect();
    let exact = format_eur(total).trim_end_matches(" €").to_string();
    quick.insert(
        0,
        (
            "Passend".to_string(),
            format!("/buchen/abbuchen?k=1&gegeben={}", urlencode(&exact)),
        ),
    );
    Ok(PayTemplate {
        ctx: page_ctx(user),
        title: "Abbuchen",
        total: format_eur(total),
        change_html: askama::Template::render(&change_view(total, &given))?,
        // A new id makes the live update replace the field instead of keeping what is being typed.
        input_id: if quick_set {
            format!("given-{}", urlencode(&given))
        } else {
            "given".to_string()
        },
        given,
        quick,
        error,
    })
}

pub async fn pay(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<GivenQuery>,
) -> Result<Response, AppError> {
    let (items, total) = load_cart(&state, &user).await?;
    if items.is_empty() {
        return Ok(Redirect::to("/buchen").into_response());
    }
    let quick_set = query.k.is_some();
    Ok(render(
        pay_page(
            total,
            query.gegeben.unwrap_or_default(),
            quick_set,
            None,
            &user,
        )
        .await?,
    )?
    .into_response())
}

/// htmx fragment: live change calculation while typing.
pub async fn change(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<GivenQuery>,
) -> Result<Html<String>, AppError> {
    let (_, total) = load_cart(&state, &user).await?;
    render(change_view(total, &query.gegeben.unwrap_or_default()))
}

#[derive(Deserialize)]
pub struct PayForm {
    csrf_token: String,
    gegeben: String,
}

pub async fn pay_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<PayForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let (items, total) = load_cart(&state, &user).await?;
    if items.is_empty() {
        return Ok(Redirect::to("/buchen").into_response());
    }
    let Some(paid) = parse_eur(&form.gegeben) else {
        let page = pay_page(
            total,
            form.gegeben,
            false,
            Some("Bitte den gegebenen Betrag eingeben, zum Beispiel 10 oder 7,50.".into()),
            &user,
        )
        .await?;
        return render_invalid(page);
    };

    // Take the cart before booking: a second tap on "Abbuchen" then finds an empty cart
    // and cannot book the same order twice. On failure the cart is restored.
    let mut taken: Vec<CartLine> = Vec::new();
    db::sessions::update_cart(&state.pool, &user.token_hash, |l| taken = std::mem::take(l)).await?;
    if taken.is_empty() {
        return Ok(Redirect::to("/buchen").into_response());
    }

    match db::sales::checkout(&state.pool, user.id, &taken, paid).await {
        Ok(receipt) => Ok(Redirect::to(&format!("/buchen/fertig/{}", receipt.id)).into_response()),
        Err(err) => {
            let restore = taken.clone();
            db::sessions::update_cart(&state.pool, &user.token_hash, move |l| *l = restore).await?;
            match err {
                CheckoutError::Db(e) => Err(e.into()),
                CheckoutError::Overflow => Err(AppError::BadRequest(err.to_string())),
                other => render_invalid(
                    pay_page(total, form.gegeben, false, Some(other.to_string()), &user).await?,
                ),
            }
        }
    }
}

pub async fn done(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let receipt = db::sales::get_receipt(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::BadRequest("Den Verkauf gibt es nicht.".into()))?;
    render(DoneTemplate {
        ctx: page_ctx(&user),
        title: "Gebucht",
        total: format_eur(receipt.total_cents),
        paid: format_eur(receipt.paid_cents),
        change: format_eur(receipt.change_cents),
        lines: receipt
            .items
            .into_iter()
            .map(|i| ReceiptLine {
                quantity: i.quantity,
                line_total: format_eur(i.unit_price_cents * i64::from(i.quantity)),
                name: i.name,
            })
            .collect(),
    })
}
