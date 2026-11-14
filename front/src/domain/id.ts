/** 生成请求幂等 ID；只用于标识操作，不携带业务含义。 */
export function makeId(prefix: string) {
  return `${prefix}_${crypto.randomUUID()}`;
}
