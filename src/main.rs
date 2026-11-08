use ariadne::config;

fn main() {
    let dir = config::data_dir();
    match config::load_de(&dir) {
        Ok(c) => {
            argos::util::define_fuso(c.tz());
            println!("{}: configs.json ok ({})", c.nome(), c.fuso);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
