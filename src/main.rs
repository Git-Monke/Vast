use std::env;
use std::sync::Once;

use spacetimedb_sdk::{DbContext, Table};
use vast_bindings::*;

fn main() {
    let host: String = env::var("SPACETIMEDB_HOST").unwrap_or("http://localhost:3000".to_string());
    let db_name: String = env::var("SPACETIMEDB_DB_NAME").unwrap_or("vast".to_string());

    static REGISTER_EMPIRE: Once = Once::new();

    let conn = DbConnection::builder()
        .with_database_name(db_name)
        .with_uri(host)
        .on_connect(|conn, _, _| {
            println!("Connected to SpacetimeDB");
            conn.subscription_builder()
                .on_applied(|ctx| {
                    println!("Subscribed to empire, building, and ship tables");
                    REGISTER_EMPIRE.call_once(|| {
                        let name = env::var("EMPIRE_NAME")
                            .unwrap_or_else(|_| "Test Empire".to_string());
                        if let Err(e) = ctx.reducers().register_empire(name) {
                            eprintln!("Failed to send register_empire: {:?}", e);
                        } else if let Err(e) = ctx.reducers().spawn_starter_ship() {
                            eprintln!("Failed to send spawn_starter_ship: {:?}", e);
                        }
                    });
                })
                .on_error(|_ctx, e| {
                    eprintln!("Subscription error: {e}");
                })
                .add_query(|q| q.from.empire())
                .add_query(|q| q.from.building())
                .add_query(|q| q.from.ship())
                .subscribe();
        })
        .on_connect_error(|_ctx, e| {
            eprintln!("Connection error: {:?}", e);
            std::process::exit(1);
        })
        .build()
        .expect("Failed to connect");

    conn.run_threaded();

    conn.db().empire().on_insert(|_ctx, empire| {
        println!(
            "Empire registered: {} — {} credits",
            empire.name, empire.credits
        );
    });

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
