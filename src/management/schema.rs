//! Native management screens. Every request is described from a verified Field API contract.
//! Display labels and editable fields are explicit; server-only fields never become inputs.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    /// Opaque, immutable metadata (e.g. an existing line ID), never a visible input.
    Hidden,
    /// Visible source text; retained in the draft but never submitted.
    ReadOnly,
    /// Photo/PDF receipts uploaded before saving; stores receiptId + fileName only.
    Attachments(&'static str),
    Text,
    Multiline,
    StringLines,
    Email,
    Integer,
    /// Human currency amount, encoded as integer minor units using the form's currency.
    Money,
    /// An exact base-10 decimal, sent as a JSON string (quantities and money).
    Decimal,
    /// An integer sent as a JSON string, without a float round-trip.
    IntegerString,
    Bool,
    Date,
    DateTime,
    Select(&'static [Choice]),
    Reference(Lookup),
    /// Lookup rows are parents; choose from the selected parent's child array.
    DependentReference {
        lookup: Lookup,
        parent_key: &'static str,
        children_key: &'static str,
    },
    Lines(&'static [Field]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub value: &'static str,
    pub label: &'static str,
}

impl Choice {
    pub const fn new(value: &'static str, label: &'static str) -> Self {
        Self { value, label }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub path: &'static str,
    pub list_key: &'static str,
    /// Dotted key; `a|b` tries explicit alternatives (e.g. parent id / child key).
    pub value_key: &'static str,
    pub label_keys: &'static [&'static str],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Empty {
    Omit,
    Null,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    /// Dotted paths support address objects; array entries are rendered recursively.
    pub key: &'static str,
    pub label: &'static str,
    pub kind: InputKind,
    pub required: bool,
    pub read_key: &'static str,
    pub default: Option<&'static str>,
    pub empty: Empty,
    pub help: &'static str,
    pub fixed_rows: bool,
}

impl Field {
    pub const fn new(key: &'static str, label: &'static str, kind: InputKind) -> Self {
        Self {
            key,
            label,
            kind,
            required: false,
            read_key: key,
            default: None,
            empty: Empty::Omit,
            help: "",
            fixed_rows: false,
        }
    }
    pub const fn text(key: &'static str, label: &'static str) -> Self {
        Self::new(key, label, InputKind::Text)
    }
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub const fn read(mut self, key: &'static str) -> Self {
        self.read_key = key;
        self
    }
    pub const fn default(mut self, value: &'static str) -> Self {
        self.default = Some(value);
        self
    }
    pub const fn empty(mut self, value: Empty) -> Self {
        self.empty = value;
        self
    }
    pub const fn help(mut self, value: &'static str) -> Self {
        self.help = value;
        self
    }
    pub const fn fixed_rows(mut self) -> Self {
        self.fixed_rows = true;
        self
    }
}

pub type PayloadHook = fn(&mut Value, &Value) -> Result<(), String>;
pub type Availability = fn(&Value) -> bool;
pub type ResponseCheck = fn(&Value) -> Result<(), String>;
/// A verified, read-only aggregate. Its JSON is interpreted using `list_key`.
pub type CollectionLoader = fn(&str, &crate::api::Tenant, &Value) -> Result<Value, String>;
/// A verified business command that needs a fresh preflight or a multi-step API contract.
pub type MutationExecutor = fn(&str, &crate::api::Tenant, &Value, &Value) -> Result<Value, String>;
#[derive(Clone, Copy, Debug)]
pub struct DocumentPrefill {
    pub entity: &'static str,
    pub schema: fn() -> Value,
    pub convert: fn(&Value) -> Result<Value, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Idempotency {
    None,
    Body(&'static str),
    Header,
}

#[derive(Clone, Copy, Debug)]
pub struct Mutation {
    pub label: &'static str,
    pub method: &'static str,
    /// `{id}` uses the resource identity; other placeholders read a dotted record key.
    /// Every replacement is escaped as one URL component.
    pub path: &'static str,
    pub fields: &'static [Field],
    pub prepare: Option<PayloadHook>,
    /// Converts server metadata into visible inputs without changing the original
    /// used for identity checks and mutation preflight.
    pub prefill: Option<fn(&Value) -> Result<Value, String>>,
    pub available: Option<Availability>,
    pub destructive: bool,
    pub explanation: &'static str,
    pub idempotency: Idempotency,
    pub capability: Option<&'static str>,
    pub actor_field: Option<&'static str>,
    pub actor_excluded_field: Option<&'static str>,
    pub actor_body_field: Option<&'static str>,
    pub response_check: Option<ResponseCheck>,
    pub executor: Option<MutationExecutor>,
    pub document: Option<DocumentPrefill>,
}

impl Mutation {
    pub const fn new(
        label: &'static str,
        method: &'static str,
        path: &'static str,
        fields: &'static [Field],
    ) -> Self {
        Self {
            label,
            method,
            path,
            fields,
            prepare: None,
            prefill: None,
            available: None,
            destructive: false,
            explanation: "",
            idempotency: Idempotency::None,
            capability: None,
            actor_field: None,
            actor_excluded_field: None,
            actor_body_field: None,
            response_check: None,
            executor: None,
            document: None,
        }
    }
    pub const fn prepare(mut self, hook: PayloadHook) -> Self {
        self.prepare = Some(hook);
        self
    }
    pub const fn prefill(mut self, hook: fn(&Value) -> Result<Value, String>) -> Self {
        self.prefill = Some(hook);
        self
    }
    pub const fn when(mut self, predicate: Availability) -> Self {
        self.available = Some(predicate);
        self
    }
    pub const fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
    pub const fn explain(mut self, text: &'static str) -> Self {
        self.explanation = text;
        self
    }
    pub const fn idempotency_body(mut self, field: &'static str) -> Self {
        self.idempotency = Idempotency::Body(field);
        self
    }
    pub const fn idempotency_header(mut self) -> Self {
        self.idempotency = Idempotency::Header;
        self
    }
    pub const fn capability(mut self, key: &'static str) -> Self {
        self.capability = Some(key);
        self
    }
    pub const fn actor_match(mut self, key: &'static str) -> Self {
        self.actor_field = Some(key);
        self
    }
    pub const fn actor_exclude(mut self, key: &'static str) -> Self {
        self.actor_excluded_field = Some(key);
        self
    }
    pub const fn actor_body(mut self, key: &'static str) -> Self {
        self.actor_body_field = Some(key);
        self
    }
    pub const fn validate_response(mut self, check: ResponseCheck) -> Self {
        self.response_check = Some(check);
        self
    }
    pub const fn execute(mut self, executor: MutationExecutor) -> Self {
        self.executor = Some(executor);
        self
    }
    pub const fn document(mut self, prefill: DocumentPrefill) -> Self {
        self.document = Some(prefill);
        self
    }
    pub fn is_available(&self, record: &Value) -> bool {
        self.available.is_none_or(|check| check(record))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pagination {
    /// API returns a complete collection, without paging.
    None,
    Offset {
        limit: usize,
    },
    Page {
        size: usize,
    },
    Cursor {
        limit: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Resource {
    pub key: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub path: &'static str,
    pub list_key: &'static str,
    pub pagination: Pagination,
    pub id_key: &'static str,
    pub title: &'static [&'static str],
    pub subtitle: &'static [&'static str],
    pub status: Option<&'static str>,
    pub detail_path: Option<&'static str>,
    pub detail_key: &'static str,
    /// Only these fields appear in detail (Japanese label, dotted read path).
    pub detail_fields: &'static [(&'static str, &'static str)],
    /// Read-only arrays displayed as labelled lines, independently of editable fields.
    pub read_lines: &'static [Field],
    pub loader: Option<CollectionLoader>,
    pub read_capability: Option<&'static str>,
    /// Match the web settings page's server-authoritative IAM admin check.
    pub admin_only: bool,
    pub create: Option<Mutation>,
    pub edit: Option<Mutation>,
    pub actions: &'static [Mutation],
    /// Native query fields; values are URL-encoded and appended to the list request.
    pub filters: &'static [Field],
    /// Exact source route, used for the implementation coverage document, never as a WebView.
    pub web_route: &'static str,
}

impl PartialEq for Resource {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for Resource {}

impl Resource {
    pub const fn new(
        key: &'static str,
        group: &'static str,
        label: &'static str,
        path: &'static str,
        list_key: &'static str,
    ) -> Self {
        Self {
            key,
            group,
            label,
            path,
            list_key,
            pagination: Pagination::None,
            id_key: "id",
            title: &["name", "title"],
            subtitle: &[],
            status: None,
            detail_path: None,
            detail_key: "",
            detail_fields: &[],
            read_lines: &[],
            loader: None,
            read_capability: None,
            admin_only: false,
            create: None,
            edit: None,
            actions: &[],
            filters: &[],
            web_route: "",
        }
    }
    pub const fn pagination(mut self, value: Pagination) -> Self {
        self.pagination = value;
        self
    }
    pub const fn id(mut self, key: &'static str) -> Self {
        self.id_key = key;
        self
    }
    pub const fn title(mut self, keys: &'static [&'static str]) -> Self {
        self.title = keys;
        self
    }
    pub const fn subtitle(mut self, keys: &'static [&'static str]) -> Self {
        self.subtitle = keys;
        self
    }
    pub const fn status(mut self, key: &'static str) -> Self {
        self.status = Some(key);
        self
    }
    pub const fn detail(mut self, path: &'static str, key: &'static str) -> Self {
        self.detail_path = Some(path);
        self.detail_key = key;
        self
    }
    pub const fn fields(mut self, fields: &'static [(&'static str, &'static str)]) -> Self {
        self.detail_fields = fields;
        self
    }
    pub const fn read_lines(mut self, fields: &'static [Field]) -> Self {
        self.read_lines = fields;
        self
    }
    pub const fn load(mut self, loader: CollectionLoader) -> Self {
        self.loader = Some(loader);
        self
    }
    pub const fn read_capability(mut self, key: &'static str) -> Self {
        self.read_capability = Some(key);
        self
    }
    pub const fn admin_only(mut self) -> Self {
        self.admin_only = true;
        self
    }
    pub const fn create(mut self, operation: Mutation) -> Self {
        self.create = Some(operation);
        self
    }
    pub const fn edit(mut self, operation: Mutation) -> Self {
        self.edit = Some(operation);
        self
    }
    pub const fn actions(mut self, operations: &'static [Mutation]) -> Self {
        self.actions = operations;
        self
    }
    pub const fn filters(mut self, fields: &'static [Field]) -> Self {
        self.filters = fields;
        self
    }
    pub const fn route(mut self, value: &'static str) -> Self {
        self.web_route = value;
        self
    }
}
