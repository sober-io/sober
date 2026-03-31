ALTER TABLE conversation_messages
  ADD COLUMN search_vector_simple tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', content)) STORED,
  ADD COLUMN search_vector_english tsvector
    GENERATED ALWAYS AS (to_tsvector('english', content)) STORED;

CREATE INDEX idx_conv_messages_search_simple ON conversation_messages
  USING GIN (search_vector_simple);
CREATE INDEX idx_conv_messages_search_english ON conversation_messages
  USING GIN (search_vector_english);
