// 资源包下载设置（模型文件不入库，应用内按需下载）。
import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { ModelPackage } from "../../api/types";

function fmtSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(0) + " MB";
  if (bytes >= 1024) return (bytes / 1024).toFixed(0) + " KB";
  return bytes + " B";
}

export default function ModelManager() {
  const [packages, setPackages] = useState<ModelPackage[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      setPackages(await api.listModelPackages());
    } catch (e) {
      console.error(e);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function download(id: string) {
    setDownloading(id);
    setError(null);
    try {
      await api.downloadModel(id);
      await refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setDownloading(null);
    }
  }

  return (
    <div className="block">
      <h3>资源包</h3>
      {error && <div className="error-text">{error}</div>}
      {packages.map(([spec, status]) => (
        <div key={spec.id} className="model-row">
          <div className="model-info">
            <div>{spec.name}</div>
            <div className="muted">
              {spec.purpose} · {fmtSize(spec.size_bytes)}
            </div>
          </div>
          {status.state === "downloaded" ? (
            <span className="model-ok">已下载</span>
          ) : (
            <button
              onClick={() => download(spec.id)}
              disabled={downloading !== null}
            >
              {downloading === spec.id ? "下载中…" : "下载"}
            </button>
          )}
        </div>
      ))}
      {packages.length === 0 && <div className="muted">加载中…</div>}
    </div>
  );
}
