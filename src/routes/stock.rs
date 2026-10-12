use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum::{Extension, Form};
use serde::Deserialize;

use crate::auth::{verify_csrf, CurrentUser};
use crate::db::categories::CategoryError;
use crate::db::products::ProductError;
use crate::db::stock::{StockError, REASON_CORRECTION, REASON_RECEIPT};
use crate::db::{self};
use crate::error::AppError;
use crate::money::{format_eur, parse_eur};
use crate::routes::{non_empty, page_ctx, render, render_invalid, CsrfForm};
use crate::templates::*;
use crate::AppState;

const MAX_QUANTITY: i32 = 100_000;

#[derive(Deserialize)]
pub struct IndexQuery {
    q: Option<String>,
    kategorie: Option<String>,
    hinweis: Option<String>,
}

fn notice_text(code: &str) -> Option<&'static str> {
    match code {
        "angelegt" => Some("Produkt angelegt."),
        "gespeichert" => Some("Änderungen gespeichert."),
        "geloescht" => Some("Produkt gelöscht."),
        "bestand" => Some("Bestand gebucht."),
        "kategorie" => Some("Kategorie gespeichert."),
        "kategorie_geloescht" => Some("Kategorie gelöscht."),
        _ => None,
    }
}

fn options(
    categories: Vec<db::categories::Category>,
    selected: Option<i64>,
) -> Vec<CategoryOption> {
    categories
        .into_iter()
        .map(|c| CategoryOption {
            selected: Some(c.id) == selected,
            id: c.id,
            name: c.name,
        })
        .collect()
}

// ---------- Products ----------

pub async fn index(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<IndexQuery>,
) -> Result<axum::response::Html<String>, AppError> {
    let q = non_empty(query.q);
    let category = non_empty(query.kategorie).and_then(|k| k.parse::<i64>().ok());
    let products = db::products::list(&state.pool, q.as_deref(), category).await?;
    let categories = db::categories::list(&state.pool).await?;
    render(StockIndexTemplate {
        ctx: page_ctx(&user),
        title: "Lager",
        tab: "produkte",
        q: q.unwrap_or_default(),
        categories: options(categories, category),
        products: products
            .into_iter()
            .map(|p| ProductView {
                id: p.id,
                name: p.name,
                category: p.category_name.unwrap_or_else(|| "–".to_string()),
                stock: p.stock,
                price: format_eur(p.price_cents),
            })
            .collect(),
        notice: query
            .hinweis
            .as_deref()
            .and_then(notice_text)
            .map(str::to_string),
    })
}

#[derive(Deserialize)]
pub struct ProductForm {
    csrf_token: String,
    name: String,
    price: String,
    category_id: String,
}

async fn product_form_page(
    state: &AppState,
    user: &CurrentUser,
    action: String,
    heading: &str,
    form: (&str, &str, Option<i64>),
    delete_url: Option<String>,
    error: Option<String>,
) -> Result<ProductFormTemplate, AppError> {
    let categories = db::categories::list(&state.pool).await?;
    Ok(ProductFormTemplate {
        ctx: page_ctx(user),
        title: "Lager",
        tab: "produkte",
        action,
        heading: heading.to_string(),
        name: form.0.to_string(),
        price: form.1.to_string(),
        categories: options(categories, form.2),
        error,
        delete_url,
    })
}

pub async fn new_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<axum::response::Html<String>, AppError> {
    let page = product_form_page(
        &state,
        &user,
        "/lager/produkte/neu".into(),
        "Neues Produkt",
        ("", "", None),
        None,
        None,
    )
    .await?;
    render(page)
}

pub async fn new_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<ProductForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let category = form.category_id.parse::<i64>().ok();
    let result = match (parse_eur(&form.price), category) {
        (None, _) => Err(ProductError::InvalidPrice),
        (_, None) => Err(ProductError::InvalidCategory),
        (Some(price), Some(cat)) => db::products::create(&state.pool, cat, &form.name, price).await,
    };
    match result {
        Ok(_) => Ok(Redirect::to("/lager?hinweis=angelegt").into_response()),
        Err(ProductError::Db(e)) => Err(e.into()),
        Err(e) => {
            let page = product_form_page(
                &state,
                &user,
                "/lager/produkte/neu".into(),
                "Neues Produkt",
                (&form.name, &form.price, category),
                None,
                Some(e.to_string()),
            )
            .await?;
            render_invalid(page)
        }
    }
}

pub async fn edit_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<axum::response::Html<String>, AppError> {
    let p = db::products::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::BadRequest("Das Produkt gibt es nicht (mehr).".into()))?;
    let price = format_eur(p.price_cents).trim_end_matches(" €").to_string();
    let page = product_form_page(
        &state,
        &user,
        format!("/lager/produkte/{id}/bearbeiten"),
        "Produkt bearbeiten",
        (&p.name, &price, p.category_id),
        Some(format!("/lager/produkte/{id}/loeschen")),
        None,
    )
    .await?;
    render(page)
}

pub async fn edit_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<ProductForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let category = form.category_id.parse::<i64>().ok();
    let result = match (parse_eur(&form.price), category) {
        (None, _) => Err(ProductError::InvalidPrice),
        (_, None) => Err(ProductError::InvalidCategory),
        (Some(price), Some(cat)) => {
            db::products::update(&state.pool, id, cat, &form.name, price).await
        }
    };
    match result {
        Ok(()) => Ok(Redirect::to("/lager?hinweis=gespeichert").into_response()),
        Err(ProductError::Db(e)) => Err(e.into()),
        Err(e) => {
            let page = product_form_page(
                &state,
                &user,
                format!("/lager/produkte/{id}/bearbeiten"),
                "Produkt bearbeiten",
                (&form.name, &form.price, category),
                Some(format!("/lager/produkte/{id}/loeschen")),
                Some(e.to_string()),
            )
            .await?;
            render_invalid(page)
        }
    }
}

pub async fn delete_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<axum::response::Html<String>, AppError> {
    let p = db::products::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::BadRequest("Das Produkt gibt es nicht (mehr).".into()))?;
    render(ConfirmDeleteTemplate {
        ctx: page_ctx(&user),
        title: "Lager",
        tab: "produkte",
        action: format!("/lager/produkte/{id}/loeschen"),
        back: format!("/lager/produkte/{id}/bearbeiten"),
        heading: format!("„{}“ löschen?", p.name),
        text: "Das Produkt verschwindet aus Lager und Kasse. Bisherige Verkäufe und Bewegungen bleiben erhalten.".into(),
        can_delete: true,
    })
}

pub async fn delete_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    match db::products::soft_delete(&state.pool, id).await {
        Ok(()) => Ok(Redirect::to("/lager?hinweis=geloescht").into_response()),
        Err(ProductError::Db(e)) => Err(e.into()),
        Err(e) => Err(AppError::BadRequest(e.to_string())),
    }
}

// ---------- Stock movements ----------

#[derive(Deserialize)]
pub struct StockForm {
    csrf_token: String,
    mode: String,
    quantity: String,
    comment: Option<String>,
}

async fn stock_form_page(
    state: &AppState,
    user: &CurrentUser,
    id: i64,
    form: (&str, &str, &str),
    error: Option<String>,
) -> Result<StockFormTemplate, AppError> {
    let p = db::products::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::BadRequest("Das Produkt gibt es nicht (mehr).".into()))?;
    Ok(StockFormTemplate {
        ctx: page_ctx(user),
        title: "Lager",
        tab: "produkte",
        action: format!("/lager/produkte/{id}/bestand"),
        product_name: p.name,
        stock: p.stock,
        mode: form.0.to_string(),
        quantity: form.1.to_string(),
        comment: form.2.to_string(),
        error,
    })
}

pub async fn stock_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<axum::response::Html<String>, AppError> {
    render(stock_form_page(&state, &user, id, ("add", "", ""), None).await?)
}

pub async fn stock_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<StockForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let comment = non_empty(form.comment.clone());
    let quantity = form
        .quantity
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|q| (1..=MAX_QUANTITY).contains(q));
    let result = match (quantity, form.mode.as_str()) {
        (None, _) => Err(StockError::InvalidQuantity),
        (Some(q), "add") => {
            db::stock::adjust(
                &state.pool,
                id,
                q,
                REASON_RECEIPT,
                comment.as_deref(),
                user.id,
            )
            .await
        }
        (Some(q), "remove") => {
            db::stock::adjust(
                &state.pool,
                id,
                -q,
                REASON_CORRECTION,
                comment.as_deref(),
                user.id,
            )
            .await
        }
        (Some(_), _) => return Err(AppError::BadRequest("Ungültige Aktion.".into())),
    };
    match result {
        Ok(_) => Ok(Redirect::to("/lager?hinweis=bestand").into_response()),
        Err(StockError::Db(e)) => Err(e.into()),
        Err(StockError::NotFound) => Err(AppError::BadRequest(StockError::NotFound.to_string())),
        Err(e) => {
            let page = stock_form_page(
                &state,
                &user,
                id,
                (&form.mode, &form.quantity, comment.as_deref().unwrap_or("")),
                Some(e.to_string()),
            )
            .await?;
            render_invalid(page)
        }
    }
}

#[derive(Deserialize)]
pub struct MovementsQuery {
    produkt: Option<String>,
}

pub async fn movements(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Query(query): Query<MovementsQuery>,
) -> Result<axum::response::Html<String>, AppError> {
    let product = non_empty(query.produkt).and_then(|p| p.parse::<i64>().ok());
    let rows = db::stock::movements(&state.pool, product, 200).await?;
    render(MovementsTemplate {
        ctx: page_ctx(&user),
        title: "Lager",
        tab: "bewegungen",
        movements: rows
            .into_iter()
            .map(|m| MovementView {
                time: m.time,
                product: m.product_name,
                positive: m.delta > 0,
                delta: if m.delta > 0 {
                    format!("+{}", m.delta)
                } else {
                    m.delta.to_string()
                },
                reason: m.reason,
                comment: m.comment.unwrap_or_default(),
                user: m.username,
            })
            .collect(),
    })
}

// ---------- Categories ----------

async fn categories_page(
    state: &AppState,
    user: &CurrentUser,
    error: Option<String>,
) -> Result<CategoriesTemplate, AppError> {
    let rows = db::categories::list_with_counts(&state.pool).await?;
    Ok(CategoriesTemplate {
        ctx: page_ctx(user),
        title: "Lager",
        tab: "kategorien",
        categories: rows
            .into_iter()
            .map(|c| CategoryView {
                id: c.id,
                name: c.name,
                product_count: c.product_count,
            })
            .collect(),
        error,
    })
}

pub async fn categories(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
) -> Result<axum::response::Html<String>, AppError> {
    render(categories_page(&state, &user, None).await?)
}

#[derive(Deserialize)]
pub struct CategoryForm {
    csrf_token: String,
    name: String,
}

async fn category_result(
    state: &AppState,
    user: &CurrentUser,
    result: Result<(), CategoryError>,
    ok_redirect: &str,
) -> Result<Response, AppError> {
    match result {
        Ok(()) => Ok(Redirect::to(ok_redirect).into_response()),
        Err(CategoryError::Db(e)) => Err(e.into()),
        Err(e) => render_invalid(categories_page(state, user, Some(e.to_string())).await?),
    }
}

pub async fn category_create(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Form(form): Form<CategoryForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let result = db::categories::create(&state.pool, &form.name)
        .await
        .map(|_| ());
    category_result(&state, &user, result, "/lager/kategorien").await
}

pub async fn category_rename(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<CategoryForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let result = db::categories::rename(&state.pool, id, &form.name).await;
    category_result(&state, &user, result, "/lager/kategorien").await
}

pub async fn category_delete_form(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
) -> Result<axum::response::Html<String>, AppError> {
    let c = db::categories::get(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::BadRequest(CategoryError::NotFound.to_string()))?;
    let can_delete = c.product_count == 0;
    render(ConfirmDeleteTemplate {
        ctx: page_ctx(&user),
        title: "Lager",
        tab: "kategorien",
        action: format!("/lager/kategorien/{id}/loeschen"),
        back: "/lager/kategorien".into(),
        heading: format!("Kategorie „{}“ löschen?", c.name),
        text: if can_delete {
            "Die Kategorie ist leer und wird gelöscht.".into()
        } else {
            format!("Die Kategorie enthält noch {} Produkt(e). Bitte erst die Produkte einer anderen Kategorie zuordnen oder löschen.", c.product_count)
        },
        can_delete,
    })
}

pub async fn category_delete_submit(
    State(state): State<AppState>,
    Extension(user): Extension<CurrentUser>,
    Path(id): Path<i64>,
    Form(form): Form<CsrfForm>,
) -> Result<Response, AppError> {
    verify_csrf(&user, &form.csrf_token)?;
    let result = db::categories::delete(&state.pool, id).await;
    category_result(&state, &user, result, "/lager/kategorien").await
}
