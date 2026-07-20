export type Store<T> = {
  get(): T;
  set(value: T): void;
  update(update: (value: T) => T): void;
  subscribe(listener: (value: T) => void): () => void;
};

export function createStore<T>(initial: T): Store<T> {
  let value = initial;
  const listeners = new Set<(value: T) => void>();

  const publish = () => {
    for (const listener of listeners) listener(value);
  };

  return {
    get: () => value,
    set(next) {
      value = next;
      publish();
    },
    update(update) {
      value = update(value);
      publish();
    },
    subscribe(listener) {
      listeners.add(listener);
      listener(value);
      return () => listeners.delete(listener);
    },
  };
}
