use anyhow::Context;
use serde::Serialize;
use spin_sdk::{
    http::{Params, Request, Response},
    http_component, http_router,
    sqlite::{self, Connection, Statement},
};

/// A simple Spin HTTP component.
#[http_component]
fn handle_todo(req: Request) -> anyhow::Result<Response> {
    println!("Handling");
    let router = http_router! {
        GET "/api/todos" => get_todos,
        POST "/api/todos/create" => create_todo,
        _   "/*"             => |req, _params| {
            println!("No handler for {} {}", req.uri(), req.method());
            Ok(http::Response::builder()
                .status(http::StatusCode::NOT_FOUND)
                .body(Some("{\"error\":\"not_found\"}".into()))
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
        println!("{today}");
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
    let statement = Statement::prepare(&format!("SELECT * FROM todos {w};"), &[])?;

    let conn = Connection::open()?;
    let todos = conn
        .query(&statement)?
        .into_iter()
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
        create.description.as_str(),
        format.as_deref().unwrap_or("NULL"),
    ];
    let statement = Statement::prepare(
        "INSERT INTO todos (description, due_date) VALUES(?, ?) RETURNING id;",
        params.as_slice(),
    )?;

    let conn = Connection::open()?;
    let response = conn.query(&statement)?.remove(0).values.remove(0);
    let sqlite::DataType::Int64(id) = response else { anyhow::bail!("Expected i64 got {response:?}")};
    let todo = Todo {
        id: id as u32,
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

impl TryFrom<sqlite::Row> for Todo {
    type Error = anyhow::Error;
    fn try_from(row: sqlite::Row) -> std::result::Result<Self, Self::Error> {
        let mut id = None;
        let mut description = None;
        let mut due_date = None;
        let mut starred = None;
        let mut is_completed = None;
        for (i, v) in row.values.into_iter().enumerate() {
            match (i, v) {
                (0, sqlite::DataType::Int64(i)) => id = Some(i as u32),
                (1, sqlite::DataType::Str(d)) => description = Some(d),
                (2, sqlite::DataType::Str(t)) => {
                    let format = time::format_description::parse(DATE_FORMAT)?;
                    due_date = Some(Some(
                        time::Date::parse(&t, &format)
                            .with_context(|| format!("Corrupted due date value: {t}"))?,
                    ))
                }
                (2, sqlite::DataType::Null) => {
                    due_date = Some(None);
                }
                (3, sqlite::DataType::Int64(b)) => starred = Some(b != 0),
                (4, sqlite::DataType::Int64(b)) => is_completed = Some(b != 0),
                (i, v) => anyhow::bail!("unexpected row data {i}: {v:?} "),
            }
        }
        Ok(Self {
            id: id.unwrap(),
            description: description.unwrap(),
            due_date: due_date.unwrap(),
            starred: starred.unwrap(),
            is_completed: is_completed.unwrap(),
        })
    }
}
