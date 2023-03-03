pub mod model;
mod hoprd_deployment;
mod hoprd_hoprd;
mod hoprd_ingress;
mod hoprd_jobs;
mod hoprd_secret;
mod hoprd_service;
mod hoprd_service_monitor;
mod servicemonitor;
mod operator;
mod actions;
mod utils;
mod constants;

#[tokio::main]
async fn main() {    
    operator::run_operator().await
}

