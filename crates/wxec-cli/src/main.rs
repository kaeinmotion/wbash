fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(wxec_cli::run_mcp(args));
}
