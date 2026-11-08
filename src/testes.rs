use crate::config;
use argos::util::TempDir;

#[test]
fn config_vazia_pega_os_padroes() {
    let tmp = TempDir::new("ariadne_config.");
    std::fs::write(tmp.0.join("configs.json"), "{}").unwrap();
    let c = config::load_de(&tmp.0).unwrap();
    assert_eq!(c.assistente, "a Ariadne");
    assert_eq!(c.nome(), "Ariadne");
    assert_eq!(c.fuso, "America/Sao_Paulo");
    assert_eq!(c.cred_dir, tmp.0.join("credenciais").to_string_lossy());
}

#[test]
fn config_sem_arquivo_ou_fuso_de_fora_nao_sobe() {
    let tmp = TempDir::new("ariadne_config.");
    assert!(config::load_de(&tmp.0).is_err());
    std::fs::write(tmp.0.join("configs.json"), r#"{"fuso": "Asia/Tokyo"}"#).unwrap();
    let e = config::load_de(&tmp.0).unwrap_err().to_string();
    assert!(e.contains("Asia/Tokyo"), "{e}");
    std::fs::write(tmp.0.join("configs.json"), r#"{"fuso": "America/Manaus", "assistente": "a Lia", "chave_nova": 1}"#).unwrap();
    let c = config::load_de(&tmp.0).unwrap();
    assert_eq!((c.nome(), c.tz()), ("Lia", chrono_tz::Tz::America__Manaus));
}
