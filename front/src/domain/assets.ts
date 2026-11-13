/**
 * 静态素材地址：public/ 下的文件按当前文档位置解析，子路径部署（GitHub Pages）也能取到。
 * 模板里直接写相对路径会被 Vue 当成模块导入，因此统一用这个函数在运行时拼接。
 */
export const mediaUrl = (name: string) => new URL(`media/${name}`, document.baseURI).href;
