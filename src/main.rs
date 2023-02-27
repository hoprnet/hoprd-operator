pub mod crd;
mod hoprd_service_monitor;
mod hoprd_deployment;
mod hoprd_service;
mod hoprd_secret;
mod hoprd_jobs;
mod servicemonitor;
mod operator;
mod actions;
mod utils;
mod finalizer;
mod constants;

#[tokio::main]
async fn main() {    
    operator::run_operator().await
}

