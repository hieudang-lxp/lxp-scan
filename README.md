# lxp-scan

Ask questions across all FE repos at once, from the terminal:

- **"Ai đang xài symbol này, ở đâu, truyền props gì?"** → `lxp-scan impact`
- **"Repo nào đang lệch version package lxp-common-*?"** → `lxp-scan drift`

AST-based (not grep): resolves tsconfig aliases, reads real imports, extracts
JSX props. Scans ~4,400 files in ~1 second.

## 🚀 Quick start

```bash
cd ~/Leapxpert/FE
lxp-scan drift
```

> **`command not found: lxp-scan`?** Terminal đó mở trước khi cài tool.
> Chạy `source ~/.cargo/env` một lần (hoặc mở tab terminal mới là tự có).

You should see:

```
| package                  | cic-admin-web | lxp-app-admin | lxp-web | lxp-web-client | drift |
| lxp-common-components-js | ^2.1.56       | ^3.1.32       | ^3.1.25 | ^2.0.64        | Major |
| lxp-common-constants-js  | ^1.0.13       | ^1.0.24       | ^1.0.24 | ^1.0.21        | Same  |
```

→ `Major` = có repo lệch nguyên major version (ở đây: cic-admin-web và
lxp-web-client còn ở v2).

Then try impact:

```bash
lxp-scan impact Button --from lxp-common-components-js
```

```
| repo    | file:line              | from                                        | refs | jsx | props                |
| lxp-web | src/.../LoginForm.tsx:5| lxp-common-components-js/components/Button  | 0    | 1   | className, disabled  |
...
12 usage site(s) in 12 file(s)
```

## 📖 Reading the impact table

| Column | Meaning |
|---|---|
| `file:line` | File + dòng của câu `import` (relative với repo) |
| `from` | Nguồn import sau khi resolve — package giữ nguyên, file nội bộ thành đường dẫn repo-relative (alias `utils/x` và relative `../utils/x` đều ra cùng một đường dẫn) |
| `refs` | Số lần dùng như biến/hàm/type (không tính JSX tag) |
| `jsx` | Số lần render `<Symbol ...>` |
| `props` | Các prop từng được truyền cho component (gộp từ mọi lần render trong file) |

## 🍳 Recipes

**Sắp sửa một shared component — vỡ chỗ nào?**
```bash
lxp-scan impact Toggle --from lxp-common-components-js --root ~/Leapxpert/FE
```
Nhìn cột `props` để biết prop nào đang được xài thật (đổi prop không ai truyền = an toàn).

**Sửa một util nội bộ trong lxp-app-admin:**
```bash
lxp-scan impact formatMessage --from utils/formatMessage --root ~/Leapxpert/FE
```
`--from` nhận cả đường dẫn nội bộ (đã resolve alias) lẫn tên package.

**Symbol tên phổ biến (Button, Modal...):** luôn kèm `--from`, không thì dính
các symbol trùng tên nội bộ của từng repo.

**Feed kết quả cho AI / script:**
```bash
lxp-scan impact Button --from lxp-common-components-js --root ~/Leapxpert/FE --json
```

**Nghi ngờ kết quả thiếu?** Thêm `--verbose` để xem warnings (file parse lỗi,
tsconfig hỏng...). Không có `--verbose` thì tool chỉ in một dòng
`N warning(s) suppressed`.

## 🎛 Flags

| Flag | Default | |
|---|---|---|
| `--root <dir>` | `.` | Thư mục chứa các repo (mỗi repo = thư mục con có package.json) |
| `--from <substring>` | — | (impact) Lọc theo nguồn import |
| `--json` | table | Output JSON ra stdout |
| `--verbose` | off | In warnings ra stderr |

Exit code: `0` kể cả khi 0 kết quả; `1` khi lỗi (root không tồn tại...).
Bảng ra stdout, warnings/summary ra stderr — pipe thoải mái.

## ⚠️ Known v1 limitations

- Không follow: namespace import (`import * as X`), re-export chain
  (`export { X } from`), dynamic `import()`, `require()`.
- Type-position references (dùng làm type) **được tính** vào `refs` — đổi type
  cũng là impact.
- `<Button.Icon />` tính là `refs` của `Button`, không tính `jsx`.
- `drift` bỏ qua khác biệt patch-level và skip version không parse được
  (`workspace:*`, `latest`, git URL).
- Thư mục ẩn (`.claude/worktrees/`...) bị skip — grep có thể "thấy" hit mà tool
  cố tình loại.
- tsconfig `extends` chưa được follow (chưa repo FE nào dùng).

## 🔧 Troubleshooting

| Lỗi | Fix |
|---|---|
| `command not found: lxp-scan` | Terminal mở trước khi cài → `source ~/.cargo/env` hoặc mở tab mới |
| Bảng rỗng khi biết chắc có usage | Check `--from` có đúng không; thử bỏ `--from`; thêm `--verbose` xem file có bị parse lỗi |
| Kết quả khác grep | Đọc phần limitations — thường là re-export/namespace import (tool bỏ qua) hoặc multi-line import (grep bỏ sót) |

## 🛠 Development

```bash
cd ~/tools/lxp-scan
cargo test                              # 35 tests (unit + integration trên fixtures)
cargo clippy --all-targets -- -D warnings
cargo install --path .                  # build + thay binary trong ~/.cargo/bin
cargo run --release -- drift --root ~/Leapxpert/FE   # chạy không cần install
```

## 🗺 Roadmap

- Phase 2: `lxp-scan context <symbol>` — xuất context pack (definition + usage
  excerpts) sẵn để paste cho AI agent.
