//! Conversation persistence: indexed pages, atomic retries, short deletion passes.

use rusqlite::{OptionalExtension, params};

use super::{SqliteTeamStore, backend};
use crate::application::conversation_ports::{Accepted, ConversationStore};
use crate::application::ports::Stored;
use crate::domain::conversation::{self, Conversation, Information, Message, Operation};
use crate::domain::{MemberId, Millis, ProjectId, Ref, RunId};

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS conversation_ready_runs (
 run INTEGER PRIMARY KEY REFERENCES runs(id), revision INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS conversations (
 id INTEGER PRIMARY KEY AUTOINCREMENT, project INTEGER NOT NULL REFERENCES projects(id),
 member1 INTEGER NOT NULL REFERENCES members(id), member2 INTEGER NOT NULL REFERENCES members(id),
 at_ms INTEGER NOT NULL, latest INTEGER, message_count INTEGER NOT NULL DEFAULT 0,
 deleted_at INTEGER, CHECK(member1 != member2)
) STRICT;
CREATE INDEX IF NOT EXISTS conversations_active ON conversations(id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_deleted ON conversations(id) WHERE deleted_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS conversations_member1 ON conversations(member1,id) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS conversations_member2 ON conversations(member2,id) WHERE deleted_at IS NULL;
CREATE TABLE IF NOT EXISTS conversation_messages (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 conversation INTEGER NOT NULL REFERENCES conversations(id),
 member INTEGER NOT NULL REFERENCES members(id), run INTEGER NOT NULL REFERENCES runs(id),
 client_key TEXT NOT NULL, text TEXT NOT NULL, evidence TEXT NOT NULL,
 accepted_day INTEGER NOT NULL, at_ms INTEGER NOT NULL,
 UNIQUE(conversation,member,client_key)
) STRICT;
CREATE INDEX IF NOT EXISTS messages_page ON conversation_messages(conversation,id);
-- Admission accounting contains no body, references or conversation link. Deleting history
-- cannot reset an allowance. Point lookups never count the growing message archive.
CREATE TABLE IF NOT EXISTS conversation_turn_usage (
 run INTEGER PRIMARY KEY REFERENCES runs(id), accepted INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS conversation_day_usage (
 member INTEGER NOT NULL REFERENCES members(id), accepted_day INTEGER NOT NULL,
 accepted INTEGER NOT NULL, PRIMARY KEY(member,accepted_day)
) STRICT;
CREATE TRIGGER IF NOT EXISTS messages_are_immutable BEFORE UPDATE ON conversation_messages
 BEGIN SELECT RAISE(ABORT, 'a message is never rewritten'); END;
CREATE TABLE IF NOT EXISTS conversation_operations (
 id INTEGER PRIMARY KEY AUTOINCREMENT, conversation INTEGER REFERENCES conversations(id),
 run INTEGER NOT NULL REFERENCES runs(id), operation TEXT NOT NULL, reason TEXT NOT NULL, at_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS conversation_operations_scope ON conversation_operations(conversation,id);
";

const COLUMNS: &str = "id,project,member1,member2,at_ms,latest,message_count";
#[cfg(test)]
impl SqliteTeamStore {
    pub(crate) fn conversation_audit_for_test(
        &self,
        run: RunId,
    ) -> Vec<(Option<i64>, String, String)> {
        let conn = self.conn().unwrap();
        conn.prepare("SELECT conversation,operation,reason FROM conversation_operations WHERE run=?1 ORDER BY id LIMIT 100")
            .unwrap().query_map([run.0], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
    }
}

pub(super) const MESSAGES_PAGE: &str = "SELECT id,conversation,member,run,text,evidence,at_ms
 FROM conversation_messages WHERE conversation=?1 AND id>?2 ORDER BY id LIMIT 51";

fn scope(row: &rusqlite::Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        project: ProjectId(row.get(1)?),
        participants: [MemberId(row.get(2)?), MemberId(row.get(3)?)],
        at: row.get(4)?,
        latest: row.get(5)?,
        messages: row.get(6)?,
    })
}

impl ConversationStore for SqliteTeamStore {
    fn mark_conversation_ready(&self, run: RunId, revision: i64) -> Stored<()> {
        self.conn()?.execute("INSERT INTO conversation_ready_runs(run,revision) VALUES(?1,?2) ON CONFLICT(run) DO UPDATE SET revision=excluded.revision",params![run.0,revision]).map_err(backend)?;
        Ok(())
    }

    fn conversation_ready(&self, run: RunId, revision: i64) -> Stored<bool> {
        self.conn()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversation_ready_runs WHERE run=?1 AND revision=?2)",
                params![run.0, revision],
                |r| r.get(0),
            )
            .map_err(backend)
    }

    fn conversation_operations_page(&self, after: i64) -> Stored<Vec<Operation>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT o.id,o.conversation,r.member,o.run,o.operation,o.reason,o.at_ms
            FROM conversation_operations o JOIN runs r ON r.id=o.run WHERE o.id>?1 ORDER BY o.id LIMIT 51").map_err(backend)?;
        stmt.query_map([after], |r| {
            Ok(Operation {
                id: r.get(0)?,
                conversation: r.get(1)?,
                member: MemberId(r.get(2)?),
                run: RunId(r.get(3)?),
                operation: r.get(4)?,
                reason: r.get(5)?,
                at: r.get(6)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn add_conversation(
        &self,
        project: ProjectId,
        members: [MemberId; 2],
        at: Millis,
    ) -> Stored<i64> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO conversations(project,member1,member2,at_ms) VALUES(?1,?2,?3,?4)",
            params![project.0, members[0].0, members[1].0, at],
        )
        .map_err(backend)?;
        Ok(conn.last_insert_rowid())
    }

    fn conversation(&self, id: i64) -> Stored<Option<Conversation>> {
        self.conn()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM conversations WHERE id=?1 AND deleted_at IS NULL"),
                [id],
                scope,
            )
            .optional()
            .map_err(backend)
    }

    fn conversation_page(&self, member: Option<MemberId>, after: i64) -> Stored<Vec<Conversation>> {
        let conn = self.conn()?;
        if let Some(member) = member {
            // Bound both index branches before UNION and sorting, even for a prolific member.
            let sql = format!("SELECT * FROM (
                SELECT * FROM (SELECT {COLUMNS} FROM conversations WHERE member1=?1 AND deleted_at IS NULL AND id>?2 ORDER BY id LIMIT 51)
                UNION ALL
                SELECT * FROM (SELECT {COLUMNS} FROM conversations WHERE member2=?1 AND deleted_at IS NULL AND id>?2 ORDER BY id LIMIT 51)
                ) ORDER BY id LIMIT 51");
            let mut stmt = conn.prepare(&sql).map_err(backend)?;
            return stmt
                .query_map(params![member.0, after], scope)
                .map_err(backend)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(backend);
        }
        let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM conversations WHERE deleted_at IS NULL AND id>?1 ORDER BY id LIMIT 51")).map_err(backend)?;
        stmt.query_map([after], scope)
            .map_err(backend)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(backend)
    }

    fn accept_information(&self, message: &Information) -> Stored<Accepted> {
        let mut conn = self.conn()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(backend)?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1 AND deleted_at IS NULL AND (member1=?2 OR member2=?2))",
            params![message.conversation,message.member.0],|r|r.get(0)).map_err(backend)?;
        if !exists {
            return Ok(Err("conversation unavailable"));
        }
        let evidence = Ref::kept(&message.evidence);
        let prior: Option<(i64,String,String)> = tx.query_row("SELECT id,text,evidence FROM conversation_messages WHERE conversation=?1 AND member=?2 AND client_key=?3",
            params![message.conversation,message.member.0,message.key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(backend)?;
        if let Some((id, text, refs)) = prior {
            if text == message.text && refs == evidence {
                tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'send','replayed',?3)",
                    params![message.conversation,message.run.0,message.at]).map_err(backend)?;
                tx.commit().map_err(backend)?;
                return Ok(Ok(id));
            }
            return Ok(Err("message key already belongs to different content"));
        }
        let turn: i64 = tx
            .query_row(
                "SELECT COALESCE((SELECT accepted FROM conversation_turn_usage WHERE run=?1),0)",
                [message.run.0],
                |r| r.get(0),
            )
            .map_err(backend)?;
        let day: i64 = tx.query_row("SELECT COALESCE((SELECT accepted FROM conversation_day_usage WHERE member=?1 AND accepted_day=?2),0)",
            params![message.member.0,message.day],|r|r.get(0)).map_err(backend)?;
        if usize::try_from(turn).map_err(backend)? >= conversation::PER_TURN
            || usize::try_from(day).map_err(backend)? >= conversation::PER_DAY
        {
            return Ok(Err(
                "information message limit reached: twelve per turn and sixty per day",
            ));
        }
        tx.execute("INSERT INTO conversation_turn_usage(run,accepted) VALUES(?1,1) ON CONFLICT(run) DO UPDATE SET accepted=accepted+1",[message.run.0]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_day_usage(member,accepted_day,accepted) VALUES(?1,?2,1) ON CONFLICT(member,accepted_day) DO UPDATE SET accepted=accepted+1",params![message.member.0,message.day]).map_err(backend)?;
        tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![message.conversation,message.member.0,message.run.0,message.key,message.text,evidence,message.day,message.at]).map_err(backend)?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE conversations SET latest=?2,message_count=message_count+1 WHERE id=?1",
            params![message.conversation, id],
        )
        .map_err(backend)?;
        tx.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,'send','accepted',?3)",
            params![message.conversation,message.run.0,message.at]).map_err(backend)?;
        tx.commit().map_err(backend)?;
        Ok(Ok(id))
    }

    fn message_page(&self, conversation: i64, after: i64) -> Stored<Vec<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(MESSAGES_PAGE).map_err(backend)?;
        stmt.query_map(params![conversation, after], |r| {
            Ok(Message {
                id: r.get(0)?,
                conversation: r.get(1)?,
                member: MemberId(r.get(2)?),
                run: RunId(r.get(3)?),
                text: r.get(4)?,
                evidence: Ref::read(&r.get::<_, String>(5)?),
                at: r.get(6)?,
            })
        })
        .map_err(backend)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(backend)
    }

    fn conversation_operation(
        &self,
        run: RunId,
        conversation: Option<i64>,
        operation: &str,
        reason: &str,
        at: Millis,
    ) -> Stored<()> {
        self.conn()?.execute("INSERT INTO conversation_operations(conversation,run,operation,reason,at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![conversation,run.0,operation,reason,at]).map_err(backend)?;
        Ok(())
    }

    fn delete_conversation(&self, id: i64, at: Millis) -> Stored<bool> {
        Ok(self
            .conn()?
            .execute(
                "UPDATE conversations SET deleted_at=?2 WHERE id=?1 AND deleted_at IS NULL",
                params![id, at],
            )
            .map_err(backend)?
            > 0)
    }

    fn purge_conversation_batch(&self) -> Stored<usize> {
        let mut conn = self.conn()?;
        let tx = conn.transaction().map_err(backend)?;
        let id: Option<i64> = tx
            .query_row(
                "SELECT id FROM conversations WHERE deleted_at IS NOT NULL ORDER BY id LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        let Some(id) = id else {
            return Ok(0);
        };
        let messages = tx.execute("DELETE FROM conversation_messages WHERE id IN (SELECT id FROM conversation_messages WHERE conversation=?1 ORDER BY id LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH).map_err(backend)?]).map_err(backend)?;
        let operations = tx.execute("DELETE FROM conversation_operations WHERE id IN (SELECT id FROM conversation_operations WHERE conversation=?1 ORDER BY id LIMIT ?2)",
            params![id,i64::try_from(conversation::PURGE_BATCH-messages).map_err(backend)?]).map_err(backend)?;
        let remaining: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM conversation_messages WHERE conversation=?1) OR EXISTS(SELECT 1 FROM conversation_operations WHERE conversation=?1)",
            [id],|r|r.get(0)).map_err(backend)?;
        if !remaining {
            tx.execute("DELETE FROM conversations WHERE id=?1", [id])
                .map_err(backend)?;
        }
        tx.commit().map_err(backend)?;
        Ok(messages + operations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::history_ports::HistoryStore;
    use rusqlite::StatementStatus;

    fn fixture(store: &SqliteTeamStore) {
        store.conn().unwrap().execute_batch("
            INSERT INTO members(id,name,role,share_percent,slots) VALUES(1,'Member 1','lead',25,2),(2,'Member 2','developer',40,2);
            INSERT INTO goals(id,title,reason,status) VALUES(1,'Fixture','','active');
            INSERT INTO projects(id,goal,title,reason,lead,status,created_by,created_at,updated_at) VALUES(1,1,'Fixture','',1,'active','owner',0,0);
            INSERT INTO wakes(id,member,reason,receipt,at_ms) VALUES(1,2,'downtime','queued',0);
            INSERT INTO runs(id,member,wake,status,queued_at) VALUES(1,2,1,'running',0);
        ").unwrap();
    }

    fn information(conversation: i64) -> Information {
        Information {
            conversation,
            member: MemberId(2),
            run: RunId(1),
            key: "message_key_1".into(),
            text: "Fixture evidence".into(),
            evidence: Vec::new(),
            day: 42,
            at: 42,
        }
    }

    #[test]
    fn a_message_and_its_retry_survive_an_independent_store_reopen() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = std::path::Path::new("target/conversation-tests");
        std::fs::create_dir_all(folder).unwrap();
        let path = folder.join(format!(
            "{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let path = path.to_str().unwrap();
        let id;
        let message;
        {
            let store = SqliteTeamStore::open(path).unwrap();
            fixture(&store);
            id = store
                .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
                .unwrap();
            message = store.accept_information(&information(id)).unwrap().unwrap();
            store.mark_confidential_input(RunId(1)).unwrap();
        }
        {
            let store = SqliteTeamStore::open(path).unwrap();
            assert!(store.run_read_confidential(RunId(1)).unwrap());
            assert_eq!(
                store.accept_information(&information(id)).unwrap().unwrap(),
                message
            );
            assert_eq!(store.message_page(id, 0).unwrap().len(), 1);
            assert_eq!(store.conversation(id).unwrap().unwrap().messages, 1);
            assert!(
                store
                    .conn()
                    .unwrap()
                    .execute("UPDATE conversation_messages SET text='changed'", [])
                    .is_err()
            );
        }
        // Separate connections accept the same previously unseen key concurrently: exactly
        // one immutable message and one summary increment survive both successful responses.
        let left = SqliteTeamStore::open(path).unwrap();
        let right = SqliteTeamStore::open(path).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = [left, right]
            .into_iter()
            .map(|store| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut next = information(id);
                    next.key = "concurrent_key_1".into();
                    barrier.wait();
                    store.accept_information(&next).unwrap().unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results[0], results[1]);
        let store = SqliteTeamStore::open(path).unwrap();
        assert_eq!(store.message_page(id, 0).unwrap().len(), 2);
        assert_eq!(store.conversation(id).unwrap().unwrap().messages, 2);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn daily_limit_applies_across_runs_and_does_not_charge_a_retry() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let id = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        for run in 1..=6 {
            if run > 1 {
                store.conn().unwrap().execute("INSERT INTO runs(id,member,wake,status,queued_at) VALUES(?1,2,1,'running',0)",[run]).unwrap();
            }
            for n in 0..12 {
                let mut next = information(id);
                next.run = RunId(run);
                next.key = format!("day_key_{run}_{n}");
                let accepted = store.accept_information(&next).unwrap();
                if run <= 5 {
                    assert!(accepted.is_ok());
                } else {
                    assert!(accepted.is_err());
                }
            }
        }
        let mut retry = information(id);
        retry.key = "day_key_1_0".into();
        assert!(store.accept_information(&retry).unwrap().is_ok());
        store.delete_conversation(id, 42).unwrap();
        while store.purge_conversation_batch().unwrap() > 0 {}
        retry.conversation = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 42)
            .unwrap();
        retry.run = RunId(6);
        retry.key = "after_deletion_key".into();
        assert!(
            store.accept_information(&retry).unwrap().is_err(),
            "owner deletion must not reset that member's admission limit"
        );
        retry.key = "following_day_key".into();
        retry.run = RunId(6);
        retry.day += 1;
        assert!(store.accept_information(&retry).unwrap().is_ok());
    }

    fn page_work(store: &SqliteTeamStore, conversation: i64) -> i32 {
        let conn = store.conn().unwrap();
        let mut stmt = conn.prepare(MESSAGES_PAGE).unwrap();
        let rows = stmt
            .query_map(params![conversation, 0], |r| r.get::<_, i64>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(rows.len(), 51);
        stmt.get_status(StatementStatus::VmStep)
    }

    #[test]
    fn historical_archive_keeps_real_message_pages_bounded_and_a_missing_index_is_detected() {
        let store = SqliteTeamStore::in_memory().unwrap();
        fixture(&store);
        let current = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        let old = store
            .add_conversation(ProjectId(1), [MemberId(1), MemberId(2)], 0)
            .unwrap();
        {
            let mut conn = store.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1000)
                SELECT CASE WHEN x%2=0 THEN ?2 ELSE ?1 END,2,1,printf('old_%d',x),'Old fixture','',0,0 FROM n",params![old,current]).unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<51)
                SELECT ?1,2,1,printf('current_%d',x),'Current fixture','',42,42 FROM n",[current]).unwrap();
            tx.commit().unwrap();
        }
        let small = page_work(&store, current);
        {
            let mut conn = store.conn().unwrap();
            let tx = conn.transaction().unwrap();
            tx.execute("INSERT INTO conversation_messages(conversation,member,run,client_key,text,evidence,accepted_day,at_ms)
                WITH RECURSIVE n(x) AS (VALUES(1001) UNION ALL SELECT x+1 FROM n WHERE x<100000)
                SELECT CASE WHEN x%2=0 THEN ?2 ELSE ?1 END,2,1,printf('old_%d',x),'Old fixture','',0,0 FROM n",params![old,current]).unwrap();
            tx.commit().unwrap();
        }
        let bounded = page_work(&store, current);
        assert!(bounded < 5000, "indexed page used {bounded} VM steps");
        assert!(
            bounded <= small + 100,
            "archive growth expanded the page's SQL work: {small} -> {bounded}"
        );
        assert_eq!(
            store.message_page(old, 0).unwrap().len(),
            51,
            "same-conversation history also stays paginated"
        );
        assert!(store.delete_conversation(old, 99).unwrap());
        let removed = store.purge_conversation_batch().unwrap();
        assert_eq!(removed, conversation::PURGE_BATCH);
        assert_eq!(store.message_page(current, 0).unwrap().len(), 51);
        store
            .conn()
            .unwrap()
            .execute("DROP INDEX messages_page", [])
            .unwrap();
        let broken = page_work(&store, current);
        eprintln!(
            "message page VM steps: 1000 old rows {small}, 100000 old rows {bounded}, missing index {broken}"
        );
        assert!(
            broken >= 5000,
            "broken unindexed query escaped the cost checker: {broken}"
        );
    }
}
