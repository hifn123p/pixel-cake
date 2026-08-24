// 画布占位：M1 阶段先渲染代理图与占位提示，M2 起替换为 WebGL2 即时预览。
// 设计（文档 §4.8）：参数变更先在此处代理图即时渲染，提交/导出时才走引擎重算。

interface Props {
  photoName: string | null;
  progress: number | null;
}

export default function Canvas({ photoName, progress }: Props) {
  return (
    <div className="canvas">
      {photoName ? (
        <div className="canvas-image">
          <span className="canvas-hint">{photoName}</span>
          {progress !== null && (
            <div className="progress">
              <div className="progress-bar" style={{ width: `${progress}%` }} />
            </div>
          )}
        </div>
      ) : (
        <div className="canvas-empty">
          <p>导入照片开始编辑</p>
          <p className="muted">WebGL2 实时预览引擎将在 M2 接入</p>
        </div>
      )}
    </div>
  );
}
