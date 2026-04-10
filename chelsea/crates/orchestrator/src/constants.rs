use uuid::Uuid;

pub fn orch_chelsea_firstboot_url(id: &Uuid) -> String {
    format!("/chelsea/firstboot/{id}")
}
