import type { components } from './schema';

type Schemas = components['schemas'];

export type InboxItem = Schemas['InboxItem'];
export type TaskProgress = Schemas['TaskProgress'];
export type JobState = Schemas['JobState'];
export type LogLine = Schemas['LogLine'];
export type SeriesRef = Schemas['SeriesRef'];
