use windows::Win32::System::{ProcessStatus::K32EmptyWorkingSet, Threading::GetCurrentProcess};

pub fn trim_working_set() {
    unsafe {
        let _ = K32EmptyWorkingSet(GetCurrentProcess());
    }
}
