fn main() {
    let mut port = 4317u16;
    let mut open = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--open" => open = true,
            "--port" => {
                port = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0);
                if port == 0 {
                    eprintln!("端口无效。");
                    std::process::exit(1);
                }
            }
            "--help" | "-h" => {
                println!("shimeji-editor [--port 4317] [--open]");
                return;
            }
            _ => {
                eprintln!("未知参数：{arg}");
                std::process::exit(1);
            }
        }
    }
    if let Err(error) = shimeji_editor::serve(port, open) {
        eprintln!("无法启动编辑器：{error}。可用 --port 指定其他端口。");
        std::process::exit(1);
    }
}
