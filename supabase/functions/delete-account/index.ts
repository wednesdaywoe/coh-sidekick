/**
 * Supabase Edge Function: delete-account
 *
 * Deletes the signed-in caller's account and everything tied to it: the preview
 * images of their published builds, the builds, then the sign-in record itself.
 * Profile, favourites and saved layout go with the sign-in record (ON DELETE
 * CASCADE). Builds are deleted here explicitly too, so this works with or
 * without the shared_builds cascade.
 *
 * Deploy with: supabase functions deploy delete-account
 */

import { createClient } from 'https://esm.sh/@supabase/supabase-js@2';

import { classifyCaller, type Verdict } from '../_shared/caller-identity.ts';
import { previewObjectPath } from '../_shared/preview-visibility.ts';

const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Headers': 'authorization, x-client-info, apikey, content-type',
};

// The body must say this, so a stray call with a valid token can't erase anyone.
const CONFIRMATION = 'DELETE';
const PAGE = 1000;

const json = (body: unknown, status: number) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { ...corsHeaders, 'Content-Type': 'application/json' },
  });

Deno.serve(async (req: Request) => {
  if (req.method === 'OPTIONS') {
    return new Response('ok', { headers: corsHeaders });
  }

  try {
    const supabaseUrl = Deno.env.get('SUPABASE_URL')!;
    const supabaseServiceKey = Deno.env.get('SUPABASE_SERVICE_ROLE_KEY')!;
    const supabase = createClient(supabaseUrl, supabaseServiceKey);

    const caller = await classifyCaller(req.headers.get('authorization'), async (token): Promise<Verdict> => {
      const { data, error } = await supabase.auth.getUser(token);
      if (error) {
        return { outcome: error.name === 'AuthRetryableFetchError' ? 'unreachable' : 'rejected' };
      }
      return data.user ? { outcome: 'user', userId: data.user.id } : { outcome: 'rejected' };
    });
    if (caller.kind === 'anonymous') return json({ error: 'Sign in to delete your account' }, 401);
    if (caller.kind === 'unverified') return json({ error: caller.detail }, 401);
    const userId = caller.userId;

    const body = await req.json().catch(() => ({}));
    if (body?.confirm !== CONFIRMATION) {
      return json({ error: `Send {"confirm":"${CONFIRMATION}"} to delete this account` }, 400);
    }

    // Every build id the account owns, a page at a time.
    const ids: string[] = [];
    for (let from = 0; ; from += PAGE) {
      const { data, error } = await supabase
        .from('shared_builds')
        .select('id')
        .eq('user_id', userId)
        .order('id')
        .range(from, from + PAGE - 1);
      if (error) {
        console.error('Build list error:', error);
        return json({ error: 'Could not read your builds; nothing was deleted' }, 500);
      }
      ids.push(...data.map((row: { id: string }) => row.id));
      if (data.length < PAGE) break;
    }

    // Images first: once the rows are gone, nothing remembers which images were theirs.
    // Removing an image that was never rendered is a no-op.
    for (let i = 0; i < ids.length; i += PAGE) {
      const { error } = await supabase.storage
        .from('build-previews')
        .remove(ids.slice(i, i + PAGE).map(previewObjectPath));
      if (error) {
        console.error('Preview removal error:', error);
        return json({ error: 'Could not remove your build images; nothing else was deleted' }, 500);
      }
    }

    const { error: buildsError } = await supabase.from('shared_builds').delete().eq('user_id', userId);
    if (buildsError) {
      console.error('Build delete error:', buildsError);
      return json({ error: 'Could not delete your builds; your account is still there' }, 500);
    }

    const { error: userError } = await supabase.auth.admin.deleteUser(userId);
    if (userError) {
      console.error('User delete error:', userError);
      return json({ error: 'Your builds were deleted, but the account was not. Please try again.' }, 500);
    }

    return json({ success: true, builds_deleted: ids.length }, 200);
  } catch (e) {
    console.error('Unexpected error:', e);
    return json({ error: 'Internal server error' }, 500);
  }
});
