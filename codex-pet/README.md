# 在 Codex 中使用“也祝”

此目录提供符合 Codex 自定义宠物规范的“也祝”动画包。图集包含空闲、左右移动、挥手、跳跃、失败、等待用户输入、执行任务和审查结果 9 种状态。

## 安装

安装时只需要复制 `yezhu/pet.json` 和 `yezhu/spritesheet.webp`。

Windows PowerShell：

```powershell
$petTarget = if ($env:CODEX_HOME) {
  Join-Path $env:CODEX_HOME "pets\yezhu"
} else {
  Join-Path $env:USERPROFILE ".codex\pets\yezhu"
}
New-Item -ItemType Directory -Force -Path $petTarget | Out-Null
Copy-Item .\yezhu\pet.json, .\yezhu\spritesheet.webp -Destination $petTarget -Force
```

macOS 或 Linux：

```bash
pet_target="${CODEX_HOME:-$HOME/.codex}/pets/yezhu"
mkdir -p "$pet_target"
cp ./yezhu/pet.json ./yezhu/spritesheet.webp "$pet_target/"
```

复制完成后重启 Codex，在宠物选择器中选择“也祝”。`preview.png` 是九种状态的联系表，`validation.json` 是图集结构与透明像素验证报告，它们不需要复制到 Codex 配置目录。

## 图集规范

- WebP RGBA，尺寸 `1536×1872`
- 8 列 × 9 行，每格 `192×208`
- 未使用单元格完全透明
- 透明像素无 RGB 残留
- `hatch-pet` 几何检查：0 错误、0 警告

角色素材随本仓库按根目录 [LICENSE](../LICENSE) 开源。
