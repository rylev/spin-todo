use anyhow::Context;
use serde::Serialize;
use spin_sdk::{
    http::{Params, Request, Response},
    http_component, http_router,
    sqlite::{self, Connection},
};

/// A simple Spin HTTP component.
#[http_component]
fn handle_todo(req: Request) -> anyhow::Result<Response> {
    let router = http_router! {
        GET "/api/todos" => get_todos,
        POST "/api/todos/create" => create_todo,
        _   "/*"             => |req, _params| {
            println!("No handler for {} {}", req.uri(), req.method());
            Ok(http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .body(Some(serde_json::json!({"error":"not_found"}).to_string().into()))
                .unwrap())
        }
    };
    router.handle(req)
}

/*
This is all assumes the following table has been created in the sqlite database already:
CREATE TABLE todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    description TEXT NOT NULL,
    due_date DATE,
    starred BOOLEAN DEFAULT 0,
    is_completed BOOLEAN DEFAULT 0
);
 */

const DATE_FORMAT: &str = &"[year]-[month]-[day]";

#[derive(serde::Deserialize)]
struct GetParams {
    #[serde(default)]
    due: Option<bool>,
    #[serde(default)]
    complete: Option<bool>,
}

pub fn get_todos(req: Request, _params: Params) -> anyhow::Result<Response> {
    let query = req.uri().query().unwrap_or_default();
    let params: GetParams = serde_qs::from_str(query)?;
    let due_date = params.due.map(|due| {
        let format = time::format_description::parse(DATE_FORMAT).unwrap();
        let today = time::OffsetDateTime::now_utc()
            .date()
            .format(&format)
            .unwrap();
        if due {
            format!("due_date <= '{today}'")
        } else {
            format!("(due_date > '{today}' OR due_date is NULL)")
        }
    });

    let incomplete = params.complete.map(|complete| {
        if complete {
            "is_completed == TRUE"
        } else {
            "is_completed == FALSE"
        }
    });

    let w = match (due_date, incomplete) {
        (Some(due_date), Some(incomplete)) => format!("WHERE {due_date} AND {incomplete}"),
        (Some(due_date), None) => format!("WHERE {due_date}"),
        (None, Some(incomplete)) => format!("WHERE {incomplete}"),
        (None, None) => String::new(),
    };

    let conn = Connection::open("default")?;
    let todos = conn
        .query(&format!("SELECT * FROM todos {w};"), &[])?
        .rows()
        .map(|r| -> anyhow::Result<Todo> { r.try_into() })
        .collect::<anyhow::Result<Vec<Todo>>>()?;

    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(Some(serde_json::to_vec(&todos)?.into()))
        .unwrap())
}

#[derive(serde::Deserialize)]
struct CreateParams {
    description: String,
    due_date: Option<time::Date>,
}

pub fn create_todo(req: Request, _params: Params) -> anyhow::Result<Response> {
    let create: CreateParams = serde_json::from_slice(
        req.body()
            .as_ref()
            .map(|b| -> &[u8] { &*b })
            .unwrap_or_default(),
    )?;
    let format = time::format_description::parse(DATE_FORMAT)?;
    let format = create.due_date.map(|d| d.format(&format).unwrap());
    let params = [
        sqlite::ValueParam::Text(&create.description),
        format
            .as_deref()
            .map(|s| sqlite::ValueParam::Text(s))
            .unwrap_or(sqlite::ValueParam::Null),
    ];

    let conn = Connection::open("default")?;
    let response = &conn.query(
        "INSERT INTO todos (description, due_date) VALUES(?, ?) RETURNING id;",
        params.as_slice(),
    )?.rows;
    let Some(id) = response.get(0) else { anyhow::bail!("Expected number got {response:?}")};
    let todo = Todo {
        id: id.get(0).unwrap(),
        description: create.description,
        due_date: create.due_date,
        starred: false,
        is_completed: false,
    };

    Ok(http::Response::builder()
        .status(http::StatusCode::OK)
        .body(Some(serde_json::to_vec(&todo)?.into()))
        .unwrap())
}

#[derive(Serialize)]
struct Todo {
    id: u32,
    description: String,
    due_date: Option<time::Date>,
    starred: bool,
    is_completed: bool,
}

impl <'a> TryFrom<sqlite::Row<'a>> for Todo {
    type Error = anyhow::Error;
    fn try_from(row: sqlite::Row<'a>) -> std::result::Result<Self, Self::Error> {
        let id = row.get("id").context("row has no id")?;
        let description: &str = row.get("description").context("row has no description")?;
        let due_date = row.get::<&str>("due_date");
        let format = time::format_description::parse(DATE_FORMAT)?;
        let due_date = due_date
            .map(|dd| time::Date::parse(dd, &format))
            .transpose()
            .context("due_date is in wrong format")?;
        let starred = row.get("starred").context("row has no starred")?;
        let is_completed = row.get("is_completed").context("row has no is_completed")?;
        Ok(Self {
            id,
            description: description.to_owned(),
            due_date,
            starred,
            is_completed,
        })
    }
}
