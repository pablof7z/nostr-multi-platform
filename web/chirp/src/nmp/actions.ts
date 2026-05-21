export type PublishNoteAction = {
  PublishNote: {
    content: string;
    reply_to_id: string | null;
    target: "Auto";
  };
};

export function publishNoteAction(content: string, replyToId: string | null = null): PublishNoteAction {
  return {
    PublishNote: {
      content,
      reply_to_id: replyToId,
      target: "Auto",
    },
  };
}
