use dto_lib::chelsea_server2::system::SystemTelemetryResponse;

pub trait SystemTelemetryResponseAvgValueExt {
    fn average_value(&self) -> f64;
}

impl SystemTelemetryResponseAvgValueExt for SystemTelemetryResponse {
    fn average_value(&self) -> f64 {
        let total_vm_memory = self.ram.vm_mib_total;
        let vm_memory_used = total_vm_memory - self.ram.vm_mib_available;

        let total_vm_cpu = self.cpu.vcpu_count_total;
        let vm_cpu_used = total_vm_cpu - self.cpu.vcpu_count_vm_available as u64;

        let total_disk = self.fs.mib_total;
        let disk_used = total_disk - self.fs.mib_available;

        let cpu_ratio = vm_cpu_used as f64 / total_vm_cpu as f64;
        let mem_ratio = vm_memory_used as f64 / total_vm_memory as f64;

        let disk_ratio = disk_used as f64 / total_disk as f64;

        let ratio = (cpu_ratio + mem_ratio + disk_ratio) / 3.0;

        ratio
    }
}
