// Restores a native snapshot sent with each repeated snapshot once
// (`review::interned`): `{"$shared": id}` becomes the shared value.
type Envelope = { interned: unknown; shared: Record<string, unknown> };

const isEnvelope = (value: unknown): value is Envelope =>
  typeof value === "object" &&
  value !== null &&
  "interned" in value &&
  "shared" in value;

export function restoreShared<T>(value: unknown): T {
  if (!isEnvelope(value)) return value as T;
  const shared = value.shared;
  const restore = (node: unknown): unknown => {
    if (Array.isArray(node)) {
      for (let index = 0; index < node.length; index += 1)
        node[index] = restore(node[index]);
      return node;
    }
    if (typeof node !== "object" || node === null) return node;
    const record = node as Record<string, unknown>;
    const keys = Object.keys(record);
    if (keys.length === 1 && keys[0] === "$shared") {
      const id = String(record.$shared);
      if (!(id in shared)) throw new Error("Missing shared snapshot value.");
      return shared[id];
    }
    for (const key of keys) record[key] = restore(record[key]);
    return record;
  };
  for (const id of Object.keys(shared)) shared[id] = restore(shared[id]);
  return restore(value.interned) as T;
}
