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

use std::future::{ready, Future};
use std::path::{Path, PathBuf};

/// 异步文件对话框的 no-op 替身(上游 `rfd::AsyncFileDialog` 面)。
///
/// builder 方法仅留存用于 log 的请求要素;四个 pick 全部立即解析为 `None`
/// ——用户取消语义,ui-egui 调用方对取消路径已有处理(直接返回)。
/// 签名逐一对照上游 0.17.2(registry `src/file_dialog.rs`)。
#[derive(Default)]
pub struct AsyncFileDialog {
    filter_names: Vec<String>,
    title: Option<String>,
    file_name: Option<String>,
}

impl AsyncFileDialog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_filter(mut self, name: impl Into<String>, _extensions: &[impl ToString]) -> Self {
        // 扩展名列表不弹窗无处消费,仅记过滤器名(请求要素)。
        self.filter_names.push(name.into());
        self
    }

    pub fn set_file_name(mut self, file_name: impl Into<String>) -> Self {
        self.file_name = Some(file_name.into());
        self
    }

    pub fn set_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// 请求要素落一条 log(info 级:ohos 面这是设计内 known-gap 行为,非异常)。
    fn log_declined(&self, kind: &str) {
        log::info!(
            "rfd-ohos stub: {kind} dialog declined (title={:?}, filters=[{}], file_name={:?}) \
             — no rfd backend on OpenHarmony",
            self.title,
            self.filter_names.join(", "),
            self.file_name,
        );
    }

    pub fn pick_file(self) -> impl Future<Output = Option<FileHandle>> {
        self.log_declined("pick_file");
        ready(None)
    }

    pub fn pick_files(self) -> impl Future<Output = Option<Vec<FileHandle>>> {
        self.log_declined("pick_files");
        ready(None)
    }

    pub fn pick_folder(self) -> impl Future<Output = Option<FileHandle>> {
        self.log_declined("pick_folder");
        ready(None)
    }

    pub fn save_file(self) -> impl Future<Output = Option<FileHandle>> {
        self.log_declined("save_file");
        ready(None)
    }
}

/// 对话框返回文件的替身(上游 `rfd::FileHandle` 面):仅 `path()`。
/// stub 的 pick 全部返回 `None`,运行期不存在构造点;字段保持上游同形
/// (`PathBuf` 私有元组),derive 与上游一致(Clone)。
#[derive(Clone)]
pub struct FileHandle(PathBuf);

impl FileHandle {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

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
