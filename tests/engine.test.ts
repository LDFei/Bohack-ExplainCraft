import test from 'node:test'
import assert from 'node:assert/strict'
import { advance, answerJob, cancelJob, createRecord, patchDraft, retryJob, seedDatabase, startJob, StudioError } from '../src/domain/engine.ts'
import { copy } from '../src/domain/content.ts'
import type { MessageInput, ProjectRecord } from '../src/domain/types.ts'

const time = Date.parse('2026-11-13T09:00:00Z')
function input(record: ProjectRecord, text = '做一个30秒的咖啡店宣传片'): MessageInput {
  return { intent: record.versions.length ? 'revise' : 'generate', text, asset_ids: [], expected_draft_revision: record.project.draft_revision, base_video_version_id: record.project.latest_video_version_id }
}
function throwsCode(action: () => unknown, code: string) { assert.throws(action, error => error instanceof StudioError && error.code === code) }

test('one sentence produces a playable, immutable version with synchronized draft revision', () => {
  const record = createRecord(time)
  const job = startJob(record, input(record), time)
  advance(record, time + 11000)
  assert.equal(job.status, 'succeeded')
  assert.equal(record.versions.length, 1)
  assert.equal(record.project.active_job_id, null)
  assert.equal(record.project.draft_dirty, false)
  assert.equal(record.versions[0]!.source_draft_revision, record.project.draft_revision)
  const frozen = copy(record.versions[0]!.snapshot)
  record.draft.audio.speech_rate = 1.8
  assert.deepEqual(record.versions[0]!.snapshot, frozen)
})

test('cancellation wins over a late completion and preserves the previous video', () => {
  const record = seedDatabase(time).projects[0]!
  const oldId = record.project.latest_video_version_id
  const job = startJob(record, input(record, '第二个镜头换成咖啡特写'), time)
  advance(record, time + 4000)
  cancelJob(record, job.id, time + 4100)
  advance(record, time + 60000)
  assert.equal(job.status, 'cancelled')
  assert.equal(record.versions.length, 1)
  assert.equal(record.project.latest_video_version_id, oldId)
})

test('a committed completion cannot be changed into cancellation', () => {
  const record = createRecord(time)
  const job = startJob(record, input(record), time)
  advance(record, time + 11000)
  throwsCode(() => cancelJob(record, job.id, time + 12000), 'JOB_ALREADY_FINISHED')
  assert.equal(job.status, 'succeeded')
})

test('concurrent generation and stale manual edits are rejected', () => {
  const record = createRecord(time)
  const request = input(record)
  const job = startJob(record, request, time)
  throwsCode(() => startJob(record, request, time + 1), 'PROJECT_BUSY')
  advance(record, time + 11000)
  throwsCode(() => patchDraft(record, { finishing: record.draft.finishing }, 1, time + 12000), 'REVISION_CONFLICT')
  throwsCode(() => startJob(record, { ...input(record), base_video_version_id: 'another-version' }, time + 12000), 'BASE_VERSION_MISMATCH')
  assert.equal(job.status, 'succeeded')
})

test('failure keeps checkpoints; retry creates a new task without duplicating the user message', () => {
  const record = createRecord(time)
  const failed = startJob(record, input(record), time, 'failure')
  advance(record, time + 5000)
  assert.equal(failed.status, 'failed')
  assert.ok(record.draft.script)
  const userCount = record.messages.filter(message => message.role === 'user').length
  const retry = retryJob(record, failed.id, record.project.draft_revision, time + 6000)
  assert.notEqual(retry.id, failed.id)
  assert.equal(retry.parent_job_id, failed.id)
  assert.equal(record.messages.filter(message => message.role === 'user').length, userCount)
  advance(record, time + 18000)
  assert.equal(retry.status, 'succeeded')
  assert.equal(record.versions.length, 1)
})

test('editing after failure invalidates the old retry input', () => {
  const record = createRecord(time)
  const job = startJob(record, input(record), time, 'failure')
  advance(record, time + 5000)
  patchDraft(record, { finishing: { ...record.draft.finishing, subtitles_enabled: false } }, record.project.draft_revision, time + 6000)
  throwsCode(() => retryJob(record, job.id, record.project.draft_revision, time + 7000), 'RETRY_INPUT_CHANGED')
})

test('answer resumes the same task, while repeated or expired answers cannot restart it', () => {
  const record = createRecord(time)
  const job = startJob(record, input(record), time, 'question')
  advance(record, time + 3000)
  assert.equal(job.status, 'waiting_input')
  const questionId = job.question!.id
  answerJob(record, job.id, questionId, '暖日咖啡', time + 4000)
  throwsCode(() => answerJob(record, job.id, questionId, '再回答一次', time + 5000), 'QUESTION_NOT_ACTIVE')
  advance(record, time + 15000)
  assert.equal(job.status, 'succeeded')
  assert.equal(record.jobs.length, 1)
})

test('an unanswered question times out instead of leaving the project busy forever', () => {
  const record = createRecord(time)
  const job = startJob(record, input(record), time, 'question')
  advance(record, time + 3000)
  const expiration = Date.parse(job.question!.expires_at)
  advance(record, expiration + 1)
  assert.equal(job.status, 'failed')
  assert.equal(job.error?.code, 'INPUT_TIMEOUT')
  assert.equal(record.project.active_job_id, null)
})

test('discussion does not modify the draft or create a video version', () => {
  const record = seedDatabase(time).projects[0]!
  const draft = copy(record.draft), revision = record.project.draft_revision
  const job = startJob(record, { ...input(record, '如何安排开头？'), intent: 'chat', base_video_version_id: null }, time)
  advance(record, time + 3000)
  assert.equal(job.status, 'succeeded')
  assert.deepEqual(record.draft, draft)
  assert.equal(record.project.draft_revision, revision)
  assert.equal(record.versions.length, 1)
})

test('saved script is preserved and its invalidated storyboard is rebuilt on update', () => {
  const record = seedDatabase(time).projects[0]!
  patchDraft(record, { script: { title: '新的故事', text: '清晨，第一缕阳光。\n咖啡香气升起。' } }, record.project.draft_revision, time)
  assert.equal(record.draft.shots.length, 0)
  assert.equal(record.project.draft_dirty, true)
  const job = startJob(record, input(record, '按当前已保存脚本更新视频'), time + 1000)
  advance(record, time + 12000)
  assert.equal(job.status, 'succeeded')
  assert.equal(record.versions[1]!.snapshot.script!.title, '新的故事')
  assert.equal(record.versions[1]!.snapshot.shots.length, 2)
  assert.equal(record.versions[0]!.snapshot.shots.length, 4)
})

test('missing assets cannot silently enter a generation request', () => {
  const record = createRecord(time)
  throwsCode(() => startJob(record, { ...input(record), asset_ids: ['someone-elses-file'] }, time), 'ASSET_NOT_READY')
  assert.equal(record.jobs.length, 0)
  assert.equal(record.messages.length, 0)
})
