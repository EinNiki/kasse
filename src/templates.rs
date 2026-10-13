use askama::Template;

/// Data every logged-in page needs for the header (user name, logout form).
#[derive(Clone)]
pub struct Ctx {
    pub username: String,
    pub csrf: String,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub csrf: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    /// Headline split around the emphasised word: pre + <em>em</em> + post.
    pub headline: (&'static str, &'static str, &'static str),
    pub is_admin: bool,
    pub revenue: String,
    pub sales_count: i64,
    pub product_count: i64,
    pub sold_out_count: i64,
}

// ---------- Lager ----------

pub struct CategoryOption {
    pub id: i64,
    pub name: String,
    pub selected: bool,
}

pub struct ProductView {
    pub id: i64,
    pub name: String,
    pub category: String,
    pub stock: i32,
    pub unlimited: bool,
    pub price: String,
}

#[derive(Template)]
#[template(path = "stock/index.html")]
pub struct StockIndexTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub q: String,
    pub categories: Vec<CategoryOption>,
    pub products: Vec<ProductView>,
    pub notice: Option<String>,
}

#[derive(Template)]
#[template(path = "stock/product_form.html")]
pub struct ProductFormTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub action: String,
    pub heading: String,
    pub name: String,
    pub price: String,
    pub unlimited: bool,
    pub categories: Vec<CategoryOption>,
    pub error: Option<String>,
    pub delete_url: Option<String>,
}

#[derive(Template)]
#[template(path = "stock/stock_form.html")]
pub struct StockFormTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub action: String,
    pub product_name: String,
    pub stock: i32,
    pub mode: String,
    pub quantity: String,
    pub comment: String,
    pub error: Option<String>,
}

#[derive(Template)]
#[template(path = "stock/delete.html")]
pub struct ConfirmDeleteTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub action: String,
    pub back: String,
    pub heading: String,
    pub text: String,
    pub can_delete: bool,
}

pub struct MovementView {
    pub time: String,
    pub product: String,
    pub delta: String,
    pub positive: bool,
    pub reason: String,
    pub comment: String,
    pub user: String,
}

#[derive(Template)]
#[template(path = "stock/movements.html")]
pub struct MovementsTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub movements: Vec<MovementView>,
}

pub struct CategoryView {
    pub id: i64,
    pub name: String,
    pub product_count: i64,
}

#[derive(Template)]
#[template(path = "stock/categories.html")]
pub struct CategoriesTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub tab: &'static str,
    pub categories: Vec<CategoryView>,
    pub error: Option<String>,
}

// ---------- Buchen ----------

pub struct CategoryTile {
    pub id: i64,
    pub name: String,
    pub product_count: i64,
}

#[derive(Template)]
#[template(path = "sales/categories.html")]
pub struct BookCategoriesTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub categories: Vec<CategoryTile>,
    pub cart_count: i32,
}

pub struct Chip {
    pub href: String,
    pub name: String,
    pub active: bool,
}

pub struct ProductTile {
    pub id: i64,
    pub name: String,
    pub price: String,
    pub stock: i32,
    pub unlimited: bool,
    pub disabled: bool,
    pub low: bool,
}

pub struct CartLineView {
    pub id: i64,
    pub name: String,
    pub quantity: i32,
    pub unit_price: String,
    pub line_total: String,
    pub can_plus: bool,
}

#[derive(Template)]
#[template(path = "sales/shop.html")]
pub struct ShopTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub search_action: String,
    pub q: String,
    pub chips: Vec<Chip>,
    pub tiles: Vec<ProductTile>,
    pub cart: Vec<CartLineView>,
    pub total: String,
    pub back: String,
    pub notice: Option<String>,
}

#[derive(Template)]
#[template(path = "sales/pay.html")]
pub struct PayTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub total: String,
    pub given: String,
    pub input_id: String,
    pub quick: Vec<(String, String)>,
    /// Pre-rendered `ChangeView`, shared with the htmx fragment.
    pub change_html: String,
    pub error: Option<String>,
}

/// Result of the change calculation, rendered inline and as htmx fragment.
#[derive(Template)]
#[template(path = "sales/_change.html")]
pub struct ChangeView {
    /// "ok", "short" or "empty"
    pub state: &'static str,
    pub amount: String,
}

pub struct ReceiptLine {
    pub quantity: i32,
    pub name: String,
    pub line_total: String,
}

#[derive(Template)]
#[template(path = "sales/done.html")]
pub struct DoneTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub total: String,
    pub paid: String,
    pub change: String,
    pub lines: Vec<ReceiptLine>,
}

// ---------- Statistik ----------

pub struct StatRow {
    pub name: String,
    pub quantity: i64,
    pub revenue: String,
    pub share: String,
}

pub struct SaleRow {
    pub time: String,
    pub user: String,
    pub items: String,
    pub total: String,
}

pub struct PeriodLink {
    pub label: &'static str,
    pub href: String,
    pub active: bool,
}

#[derive(Template)]
#[template(path = "stats.html")]
pub struct StatsTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub from: String,
    pub to: String,
    pub period_label: String,
    pub links: Vec<PeriodLink>,
    pub today: String,
    pub week: String,
    pub month: String,
    pub revenue: String,
    pub sales_count: i64,
    pub average: String,
    pub top_quantity: Vec<StatRow>,
    pub top_revenue: Vec<StatRow>,
    pub categories: Vec<StatRow>,
    pub sales: Vec<SaleRow>,
}

// ---------- Admin ----------

pub struct UserView {
    pub id: i64,
    pub username: String,
    pub created: String,
    pub active: bool,
    pub is_admin: bool,
    pub is_self: bool,
}

#[derive(Template)]
#[template(path = "admin.html")]
pub struct AdminTemplate {
    pub ctx: Option<Ctx>,
    pub title: &'static str,
    pub users: Vec<UserView>,
    pub username: String,
    pub error: Option<String>,
    pub notice: Option<String>,
}
