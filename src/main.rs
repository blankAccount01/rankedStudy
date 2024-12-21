use actix_web::{web, App, HttpServer, HttpResponse, Responder};
use actix_files as fs;
use serde::{Serialize, Deserialize};
use tokio::fs::{self as tokioFS, File};
use serde_json;
use tokio::io::AsyncWriteExt;
use std::sync::{Mutex, Arc};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct GlobalProgress {
    red: f64,
    blue: f64,
}

#[derive(Serialize, Deserialize,Debug)]
struct Points {
    team : String,
    points: f64,
    user_id:String,
    name: String,
}

struct RateLimiter {
    requests: Mutex<HashMap<String, Instant>>,
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter {
            requests: Mutex::new(HashMap::new()),
        }
    }

    fn waited_enough(&self, ip:&str, points:f64) -> bool {
        let mut requests = self.requests.lock().unwrap();
        let now = Instant::now();
        match requests.get(ip){
            Some(last_request_time) =>{
                let required_time = Duration::from_secs((points * 0.3 *60.0) as u64);
                if now.duration_since(*last_request_time) < required_time {
                    println!("User Rate Limited: {}",ip);
                    return false;
                }
            }
            None => {
                println!("No previous request from id: {}", ip);
                requests.insert(ip.to_string(), now);
                return true;
            }
        }
        requests.insert(ip.to_string(),now);
        true
    }
}

async fn initialize_database() -> GlobalProgress {
    match tokioFS::read_to_string("./data/data.json").await {
        Ok(contents) => match serde_json::from_str::<GlobalProgress>(&contents) {
            Ok(data) => data,
            Err(_) => GlobalProgress { red: 0.0, blue: 0.0 },
        },
        Err(_) => GlobalProgress { red: 0.0, blue: 0.0 },
    }
}

async fn change_points(
    rate_limiter: web::Data<Arc<RateLimiter>>,
    data_mutex: web::Data<Arc<Mutex<GlobalProgress>>>,
    points: web::Json<Points>
) -> impl Responder {
    if rate_limiter.waited_enough(&points.user_id, points.points) {

        let mut data = match data_mutex.lock() {
            Ok(data) => data,
            Err(_) => return HttpResponse::InternalServerError().body("Database not locked :("),
        };

        println!("Received data: {:?}", points);
        if points.team == "red" {
            data.red += points.points;
        } else if points.team == "blue" {
            data.blue += points.points;
        }
        match serde_json::to_string(&*data) {
            Ok(json_data) => {
                match File::create("./data/data.json").await {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(json_data.as_bytes()).await {
                            eprintln!("Failed to write to file: {}", e);
                            return HttpResponse::InternalServerError().body("Bruh database file blocked");
                        }
                        HttpResponse::Ok().body("Points added")
                    }
                    Err(e) => {
                        eprintln!("Failed to open file: {}", e);
                        HttpResponse::InternalServerError().body("Uh file failed")
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to serialize data: {}", e);
                HttpResponse::InternalServerError().body("Json failed?!?")
            }
        }
    }else {
    HttpResponse::TooManyRequests().body("Rate limit exceeded. STOP TRYING TO CHEAT.")
    }
}

async fn fetch_data(database: web::Data<Arc<Mutex<GlobalProgress>>>) -> impl Responder {
    let data = database.lock().unwrap();
    HttpResponse::Ok().json(&*data)
}

#[actix_web::main]
async fn main() -> std::io::Result<()>{

    let rate_limiter = Arc::new(RateLimiter::new());
    let database = Arc::new(Mutex::new(initialize_database().await));

    HttpServer::new(move || {
        App::new()
            .route("/", web::get().to(|| async {
                    fs::NamedFile::open("./static/index.html")
                        .map_err(|_| HttpResponse::NotFound().body("Website not found"))
                        .unwrap()
                }))
            .service(fs::Files::new("/static", "./static").index_file("index.html"))
            .route("/api/data",web::get().to(fetch_data))
            .app_data(web::Data::new(rate_limiter.clone()))
            .app_data(web::Data::new(database.clone()))
            .route("/api/kiode",web::post().to(change_points))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}