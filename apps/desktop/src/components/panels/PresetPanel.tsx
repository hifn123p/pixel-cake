// 预设面板：保存当前编辑参数为预设，一键应用。

import { useEffect, useState } from "react";
import { api } from "../../api/client";
import type { Preset, Recipe } from "../../api/types";

interface Props {
  currentRecipe: Recipe;
  onApplyRecipe: (recipe: Recipe) => void;
}

export default function PresetPanel({ currentRecipe, onApplyRecipe }: Props) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listPresets().then(setPresets).catch((e) => setError(String(e)));
  }, []);

  async function save() {
    if (!name.trim()) return;
    try {
      await api.savePreset(name.trim(), currentRecipe);
      setName("");
      setPresets(await api.listPresets());
    } catch (e) {
      setError(String(e));
    }
  }

  function apply(p: Preset) {
    try {
      const recipe = JSON.parse(p.recipe_json) as Recipe;
      onApplyRecipe(recipe);
    } catch (e) {
      setError(`预设解析失败: ${e}`);
    }
  }

  return (
    <div className="panel">
      <h3>预设</h3>
      <div className="block">
        <input
          placeholder="预设名称"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <button onClick={save} disabled={!name.trim()}>
          保存当前设置
        </button>
      </div>

      {error && <p className="error-text">{error}</p>}

      <ul className="list">
        {presets.map((p) => (
          <li key={p.id} className="row">
            <span>{p.name}</span>
            <button onClick={() => apply(p)}>应用</button>
          </li>
        ))}
        {presets.length === 0 && (
          <li className="muted">暂无预设，保存当前设置后显示在这里</li>
        )}
      </ul>
    </div>
  );
}
