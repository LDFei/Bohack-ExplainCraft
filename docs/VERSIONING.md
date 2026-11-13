# 第一版保存与后续改造

本项目通过 GitHub 保存完整源码、文档、测试、依赖锁文件、截图和演示媒体。node_modules、dist、测试临时产物和本机环境配置不提交；使用 `npm ci` 和 `npm run build` 可以重新安装依赖、生成构建文件。

浏览器里的创作记录和个人上传文件属于本机网站数据，不是项目源码，不会随 Git 上传。仓库中包含 public/media 内的全部内置演示媒体。

## 当前版本约定

| 名称 | 用途 |
| --- | --- |
| `v0.1.0` 标签 | 第一版前端演示的固定快照，保留用于找回和重新改造 |
| `main` 分支 | 稳定版本；初次上传时与第一版标签指向同一次提交 |
| `develop` 分支 | 当前工作分支，后续按自己的想法修改、提交和推送 |

第一版包括三栏 Vue 工作台、演示任务流程、素材上传、草稿与版本、视频播放下载、相关文档和测试；真实 Rust 和 AI 生成后端尚未接通。不要移动或删除 `v0.1.0` 标签，也无需为了重新开始而覆盖 main 或 develop 的历史。

下面的命令均在项目根目录执行。首次上传后，本地已经切到 develop。

## 平时保存后续修改

先查看当前分支和修改内容，确认是自己准备保存的内容，再提交：

```bash
git status
git add .
git commit -m "继续改造前端页面与创作流程"
git push origin develop
```

后续 push 更新 develop，不会改变第一版标签。提交说明可以替换成这次修改的实际内容；没有待提交修改时不需要再次 commit。

## 改造不满意，从第一版重新开始

先把当前尝试提交并推送到现有分支，保留这次尝试。然后从第一版创建另一条分支：

```bash
git fetch origin --tags
git switch -c redesign-from-v1 v0.1.0
git push -u origin redesign-from-v1
```

此时项目的受 Git 管理文件回到第一版，新改造提交到 redesign-from-v1；原来的 develop 和第一版标签都保留。第二次再从第一版尝试时，为新分支换一个未使用的名字，例如 redesign-from-v1-2。

切换到旧版本后，先运行 `npm ci` 再启动，确保本机依赖与该版本的 package-lock.json 一致。被忽略的文件和浏览器网站数据不随 Git 分支切换回滚。

## 回到之前的改造

先保存当前分支的修改，再切回 develop：

```bash
git switch develop
npm ci
npm run dev
```

## 从 GitHub 下载第一版

在仓库页面的 Tags 中选择 `v0.1.0`，可以浏览或下载该版本。也可以在一份新的目录中克隆第一版并新建自己的分支：

```bash
git clone --branch v0.1.0 https://github.com/LDFei/Bohack-ExplainCraft.git Bohack-ExplainCraft-v1
cd Bohack-ExplainCraft-v1
git switch -c my-redesign
npm ci
npm run dev
```
