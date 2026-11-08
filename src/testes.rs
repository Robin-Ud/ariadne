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

// --- membros ---

use crate::membros;

#[test]
fn slug_sem_acento_nem_espaco() {
    assert_eq!(membros::slug_de("  João Pedro "), "joao-pedro");
    assert_eq!(membros::slug_de("Ana-Lú (sócia)"), "ana-lu-socia");
    assert_eq!(membros::slug_de("!!"), "");
    assert_eq!(membros::link("@EquipeBot", "abc"), "https://t.me/EquipeBot?start=abc");
}

#[test]
fn convite_pareia_uma_vez_e_vence() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new("ariadne_membros.");
    let t0 = 1_800_000_000;
    let (m, codigo) = membros::cadastrar(&tmp.0, "Fulana de Tal", Some("fulana@exemplo.com"), t0).unwrap();
    assert_eq!((m.slug.as_str(), codigo.len()), ("fulana-de-tal", 12));
    assert!(membros::cadastrar(&tmp.0, "?!", None, t0).is_err());

    assert_eq!(membros::do_convite(&tmp.0, "errado", t0), None);
    assert_eq!(membros::do_convite(&tmp.0, "", t0), None);
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + membros::CONVITE_S + 1), None, "venceu");
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + 60).as_deref(), Some("fulana-de-tal"));
    assert!(membros::convite_pendente(&tmp.0, "fulana-de-tal", t0));

    membros::parear(&tmp.0, "fulana-de-tal", 4242).unwrap();
    let p = membros::pasta(&tmp.0).join("fulana-de-tal");
    assert_eq!(std::fs::metadata(p.join("chat_id")).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(membros::do_convite(&tmp.0, &codigo, t0 + 60), None, "consumido");
    assert!(!membros::convite_pendente(&tmp.0, "fulana-de-tal", t0));

    // recadastrar: convite novo, chat_id e e-mail ficam
    let (_, novo) = membros::cadastrar(&tmp.0, "Fulana de Tal", None, t0).unwrap();
    assert_ne!(novo, codigo);
    let v = membros::listar(&tmp.0).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!((v[0].chat_id, v[0].email.as_deref()), (Some(4242), Some("fulana@exemplo.com")));
}

#[test]
fn um_chat_e_uma_pessoa() {
    let tmp = TempDir::new("ariadne_membros.");
    membros::cadastrar(&tmp.0, "Beltrano", None, 0).unwrap();
    membros::cadastrar(&tmp.0, "Beltrano Silva", None, 0).unwrap();
    membros::parear(&tmp.0, "beltrano", 7).unwrap();
    membros::parear(&tmp.0, "beltrano-silva", 7).unwrap();
    let v = membros::listar(&tmp.0).unwrap();
    assert_eq!(v.iter().map(|m| (m.slug.as_str(), m.chat_id)).collect::<Vec<_>>(), [("beltrano", None), ("beltrano-silva", Some(7))]);
}
