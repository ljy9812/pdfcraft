//! rfd-ohos — rfd 0.17 API 面的 OpenHarmony no-op stub(ohos-craft-p2 G1)。
//!
//! rfd 没有 ohos 后端。本 crate 让 `pdfcraft-ui-egui` 在 ohos 面保住全部
//! `use rfd::…` 调用点零改动:经依赖键 package 重命名被当作 `rfd` 消费。
//! 每个 dialog 请求返回 `None`(用户取消语义)并记一条 log 通知——不
//! panic、不挂起。ui-egui 的最小消费面(逐一核自源码):`AsyncFileDialog`
//! (`new`/`add_filter`/`set_title`/`set_file_name`/`pick_file`/`pick_files`/
//! `pick_folder`/`save_file`)与 `FileHandle::path`;`FileDialog` 仅出现于
//! 注释,不实现。
//!
//! 本 crate 是本地 stub 代码,不是上游 vendoring。

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};

    use super::{AsyncFileDialog, FileHandle};

    /// 以 no-op waker 单次 poll 断言 future 立即解析为 `None`:stub 不得
    /// 挂起调用方 executor——dialog 请求由用户手势触发,必须完成。
    fn assert_immediately_none<T>(future: impl Future<Output = Option<T>>) {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => assert!(
                value.is_none(),
                "ohos stub dialog 必须解析为 None(取消语义)"
            ),
            Poll::Pending => panic!("ohos stub dialog 不得挂起"),
        }
    }

    #[test]
    fn builder_chain_covers_ui_egui_surface() {
        // ui-egui 产出的完整 builder 链(add_filter ×41 / set_title ×18 /
        // set_file_name ×10;无 set_directory 等其他方法调用点)。
        let dialog = AsyncFileDialog::new()
            .add_filter("PDF 文件", &["pdf"])
            .set_title("打开")
            .set_file_name("form.pdf");
        assert_immediately_none(dialog.pick_file());
    }

    #[test]
    fn pick_file_returns_none() {
        assert_immediately_none(AsyncFileDialog::new().pick_file());
    }

    #[test]
    fn pick_files_returns_none() {
        assert_immediately_none(AsyncFileDialog::new().pick_files());
    }

    #[test]
    fn pick_folder_returns_none() {
        assert_immediately_none(AsyncFileDialog::new().pick_folder());
    }

    #[test]
    fn save_file_returns_none() {
        assert_immediately_none(AsyncFileDialog::new().save_file());
    }

    #[test]
    fn file_handle_path_roundtrip() {
        // ui-egui 对 FileHandle 的唯一调用形态:`handle.path()`(pickers.rs)。
        let path = PathBuf::from("/data/storage/el2/base/files/form.pdf");
        let handle = FileHandle(path.clone());
        assert_eq!(handle.path(), path);
    }
}
