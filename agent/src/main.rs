use ksni::TrayMethods;
struct Prototype;
impl ksni::Tray for Prototype {
    fn id(&self) -> String { "actualizaciones-prototype".into() }
    fn title(&self) -> String { "Actualizaciones (prototipo SNI)".into() }
    fn icon_name(&self) -> String { "software-update-available".into() }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::StandardItem;
        vec![StandardItem { label: "Abrir (prototipo)".into(), ..Default::default() }.into(),
             StandardItem { label: "Comprobar (prototipo)".into(), ..Default::default() }.into(),
             StandardItem { label: "Inicio automático (prototipo)".into(), ..Default::default() }.into(),
             StandardItem { label: "Salir".into(), activate: Box::new(|_| std::process::exit(0)), ..Default::default() }.into()]
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() { let _handle = Prototype.spawn().await.expect("bandeja SNI"); std::future::pending::<()>().await; }
