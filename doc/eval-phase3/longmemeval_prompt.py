def prepare_prompt(entry, retriever_type, topk_context: int, useronly: bool, history_format: str, cot: bool, tokenizer, tokenizer_backend, max_retrieval_length, merge_key_expansion_into_value, con=False, con_client=None, con_model=None):    
    if retriever_type == 'no-retrieval':
        answer_prompt_template = '{}'
        if cot:
            answer_prompt_template += 'Answer step by step.'
            
    else:
        if merge_key_expansion_into_value is None or merge_key_expansion_into_value == 'none':
            if cot:
                answer_prompt_template = 'I will give you several history chats between you and a user. Please answer the question based on the relevant chat history. Answer the question step by step: first extract all the relevant information, and then reason over the information to get the answer.\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer (step by step):'
            else:
                answer_prompt_template = 'I will give you several history chats between you and a user. Please answer the question based on the relevant chat history.\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer:'
        elif merge_key_expansion_into_value == 'merge':
            if cot:
                answer_prompt_template = 'I will give you several history chats between you and a user, as well as the relevant user facts extracted from the chat history. Please answer the question based on the relevant chat history and the user facts. Answer the question step by step: first extract all the relevant information, and then reason over the information to get the answer.\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer (step by step):'
            else:
                answer_prompt_template = 'I will give you several history chats between you and a user, as well as the relevant user facts extracted from the chat history. Please answer the question based on the relevant chat history and the user facts\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer:'
        elif merge_key_expansion_into_value == 'replace':
            if cot:
                answer_prompt_template = 'I will give you several facts extracted from history chats between you and a user. Please answer the question based on the relevant facts. Answer the question step by step: first extract all the relevant information, and then reason over the information to get the answer.\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer (step by step):'
            else:
                answer_prompt_template = 'I will give you several facts extracted from history chats between you and a user. Please answer the question based on the relevant facts.\n\n\nHistory Chats:\n\n{}\n\nCurrent Date: {}\nQuestion: {}\nAnswer:'
        else:
            raise NotImplementedError
        
    question_date_string = entry['question_date']
    question_string = entry['question']

    corpusid2date, corpusid2entry = {}, {}
    for session_date, session_id, session_entry in zip(entry['haystack_dates'], entry['haystack_session_ids'], entry['haystack_sessions']):
        corpusid2date[session_id] = session_date
        corpusid2entry[session_id] = session_entry
        for i_turn, turn_entry in enumerate(session_entry):
            corpusid2date[session_id + '_' + str(i_turn+1)] = session_date
            corpusid2entry[session_id + '_' + str(i_turn+1)] = turn_entry

    corpusid2retvalue = {}
    try:
        for ret_result_entry in entry['retrieval_results']['ranked_items']:
            corpusid2retvalue[ret_result_entry['corpus_id']] = ret_result_entry['text']
    except:
        pass
    
    retrieved_chunks = []
    # get chunks in the original order
    if retriever_type == "orig-session":   # no retrieval, session
        for session_date, session_entry in zip(entry['haystack_dates'], entry['haystack_sessions']):
            if useronly:
                retrieved_chunks.append((session_date, [x for x in session_entry if x['role'] == 'user']))
            else:
                retrieved_chunks.append((session_date, session_entry))
    elif retriever_type == "orig-turn":  # no retrieval, turn
        for session_date, session_entry in zip(entry['haystack_dates'], entry['haystack_sessions']):
            if useronly:
                retrieved_chunks += [(session_date, x) for x in session_entry if x['role'] == 'user']
            else:
                retrieved_chunks += [(session_date, x) for x in session_entry]

    # only retain oracle chunks 
    elif retriever_type == "oracle-session":   # no retrieval, session
        for session_date, session_entry in zip(entry['haystack_dates'], entry['haystack_sessions']):
            if useronly:
                retrieved_chunks.append((session_date, [x for x in session_entry if x['role'] == 'user']))
            else:
                retrieved_chunks.append((session_date, session_entry))
    elif retriever_type == "oracle-turn":  # no retrieval, turn
        for session_date, session_entry in zip(entry['haystack_dates'], entry['haystack_sessions']):
            if useronly:
                retrieved_chunks += [(session_date, x) for x in session_entry if x['role'] == 'user']
            else:
                retrieved_chunks += [(session_date, x) for x in session_entry]

    # get retrieved chunks
    elif retriever_type == "flat-turn":
        for ret_result_entry in entry['retrieval_results']['ranked_items']:
            converted_corpus_id = '_'.join(ret_result_entry['corpus_id'].replace('noans_', 'answer_').split('_')[:-1])
            converted_turn_id = int(ret_result_entry['corpus_id'].replace('noans_', 'answer_').split('_')[-1]) - 1   # we had offset one during retrieval
            # automatically expand turn into round
            try:
                cur_round_data = [corpusid2entry[converted_corpus_id][converted_turn_id]]
                converted_next_turn_id = converted_turn_id + 1
                if converted_next_turn_id < len(corpusid2entry[converted_corpus_id]):
                    cur_round_data.append(corpusid2entry[converted_corpus_id][converted_next_turn_id])
                
            except:
                continue
            
            # handle optional merging key into the value
            if merge_key_expansion_into_value is None or merge_key_expansion_into_value == 'none':
                retrieved_chunks.append((corpusid2date[converted_corpus_id], cur_round_data))
            elif merge_key_expansion_into_value == 'replace':
                retrieved_chunks.append((corpusid2date[converted_corpus_id], corpusid2retvalue[ret_result_entry['corpus_id']]))
            elif merge_key_expansion_into_value == 'merge':
                retrieved_chunks.append((corpusid2date[converted_corpus_id], corpusid2retvalue[ret_result_entry['corpus_id']], cur_round_data))
            else:
                raise NotImplementedError

        if useronly and not merge_key_expansion_into_value == 'replace':
            retrieved_chunks = [x for x in retrieved_chunks if x[-1]['role'] == 'user']     

    elif retriever_type == "flat-session":
        for ret_result_entry in entry['retrieval_results']['ranked_items']:
            # handle optional merging key into the value
            if merge_key_expansion_into_value is None or merge_key_expansion_into_value == 'none':
                if useronly:
                    retrieved_chunks.append((corpusid2date[ret_result_entry['corpus_id'].replace('noans_', 'answer_')],
                                            [x for x in corpusid2entry[ret_result_entry['corpus_id'].replace('noans_', 'answer_')] if x['role'] == 'user']))
                else:
                    retrieved_chunks.append((corpusid2date[ret_result_entry['corpus_id'].replace('noans_', 'answer_')], corpusid2entry[ret_result_entry['corpus_id'].replace('noans_', 'answer_')]))
            elif merge_key_expansion_into_value == 'replace':
                retrieved_chunks.append((corpusid2date[ret_result_entry['corpus_id'].replace('noans_', 'answer_')], corpusid2retvalue[ret_result_entry['corpus_id']]))
            elif merge_key_expansion_into_value == 'merge':
                if useronly:
                    retrieved_chunks.append((corpusid2date[ret_result_entry['corpus_id'].replace('noans_', 'answer_')], corpusid2retvalue[ret_result_entry['corpus_id']], [x for x in corpusid2entry[ret_result_entry['corpus_id'].replace('noans_', 'answer_')] if x['role'] == 'user']))
                else:
                    retrieved_chunks.append((corpusid2date[ret_result_entry['corpus_id'].replace('noans_', 'answer_')], corpusid2retvalue[ret_result_entry['corpus_id']], corpusid2entry[ret_result_entry['corpus_id'].replace('noans_', 'answer_')]))
            else:
                raise NotImplementedError

    elif retriever_type == "no-retrieval":
        retrieved_chunks = []
        
    else:
        raise NotImplementedError

    if retriever_type in ["orig-turn", "orig-session"]:
        retrieved_chunks = retrieved_chunks[-topk_context:]  # keep latest
    else:
        retrieved_chunks = retrieved_chunks[:topk_context]

    # clean up
    retrieved_chunks_cleaned = []
    for retrieved_item in retrieved_chunks:
        try:
            date, session_entry = retrieved_item
            for turn_entry in session_entry:
                if type(turn_entry) == dict and 'has_answer' in turn_entry:
                    turn_entry.pop('has_answer')
            retrieved_chunks_cleaned.append((date, session_entry))
        except:
            date, expansion_entry, session_entry = retrieved_item
            for turn_entry in session_entry:
                if type(turn_entry) == dict and 'has_answer' in turn_entry:
                    turn_entry.pop('has_answer')
            retrieved_chunks_cleaned.append((date, expansion_entry, session_entry))
    retrieved_chunks = retrieved_chunks_cleaned

    # optional: if CoN is specified, add an information extraction process before feeding into the model
    if con:
        con_prompt = "I will give you a chat history between you and a user, as well as a question from the user. Write reading notes to extract all the relevant user information relevant to answering the answer. If no relevant information is found, just output \"empty\". \n\n\nChat History:\nSession Date: {}\nSession Content:\n{}\n\nQuestion Date: {}\nQuestion: {}\nExtracted note (information relevant to answering the question):"
        retrieved_chunks_with_notes = []
        for i, cur_item in enumerate(retrieved_chunks):
            if merge_key_expansion_into_value == 'merge':
                (chunk_date, chunk_expansion_entry, chunk_entry) = cur_item
                                
            else:
                (chunk_date, chunk_entry) = cur_item
                
            kwargs = {
                'model': con_model,
                'messages':[
                    {"role": "user", "content": con_prompt.format(chunk_date, json.dumps(chunk_entry), question_date_string, question_string)}
                ],
                'n': 1,
                'temperature': 0,
                'max_tokens': 500,
            }
            completion = chat_completions_with_backoff(con_client, **kwargs) 
            cur_note = completion.choices[0].message.content.strip()
            chunk_entry_con = {'session_summary': cur_note}

            if merge_key_expansion_into_value == 'merge':
                retrieved_chunks_with_notes.append((chunk_date, chunk_expansion_entry, chunk_entry_con))
            else:
                retrieved_chunks_with_notes.append((chunk_date, chunk_entry_con))

        retrieved_chunks = retrieved_chunks_with_notes
                
    # sort sessions by their dates
    retrieved_chunks.sort(key=lambda x: x[0])
    
    history_string = ""
    for i, cur_item in enumerate(retrieved_chunks):
        if merge_key_expansion_into_value == 'merge':
            (chunk_date, chunk_expansion_entry, chunk_entry) = cur_item
        else:
            (chunk_date, chunk_entry) = cur_item

        if history_format == 'json':
            if merge_key_expansion_into_value == 'merge':
                sess_string = '\n' + json.dumps({'session_summary_facts': chunk_expansion_entry, 'original_session': chunk_entry})
            else:
                sess_string = '\n' + json.dumps(chunk_entry)
        elif history_format == 'nl':
            sess_string = ""
            if merge_key_expansion_into_value == 'merge':
                sess_string += "\n\nSession summary and facts:" + chunk_expansion_entry
            if type(chunk_entry) == list:
                for turn_entry in chunk_entry:
                    sess_string += "\n\n{}: {}".format(turn_entry['role'], turn_entry['content'].strip())
            else:
                sess_string += "{}: {}".format(chunk_entry['role'], chunk_entry['content'].strip())    
        else:
            raise NotImplementedError

        if retriever_type in ["orig-session", "flat-session", "oracle-session"]:
            history_string += '\n### Session {}:\nSession Date: {}\nSession Content:\n{}\n'.format(i+1, chunk_date, sess_string)
        elif retriever_type in ["orig-turn", "flat-turn", "oracle-turn"]:  
            # history_string += '\n### Round {}:\nDate: {}\nRound Content:\n{}\n'.format(i+1, chunk_date, sess_string)
            history_string += '\n### Session {}:\nSession Date: {}\nSession Content:\n{}\n'.format(i+1, chunk_date, sess_string)  # we include both sides right now
        elif retriever_type == "no-retrieval":
            history_string += ""
        else:
            raise NotImplementedError

    assert retriever_type == "no-retrieval" or history_string != ""
    if retriever_type == "no-retrieval":
        prompt = answer_prompt_template.format(question_string)
    else:
        # truncate history string
        if tokenizer_backend == 'openai':
            tokens = tokenizer.encode(history_string, allowed_special={'<|endoftext|>'})
            if len(tokens) > max_retrieval_length:
                print('Truncating from {} to {}'.format(len(tokens), max_retrieval_length), flush=True)
                truncated_tokens = tokens[:max_retrieval_length]
                history_string = tokenizer.decode(truncated_tokens)
        elif tokenizer_backend == 'huggingface':
            encoded_input = tokenizer(history_string, max_length=max_retrieval_length, truncation=False, return_tensors="pt")
            if len(encoded_input['input_ids'][0]) > max_retrieval_length:
                print('Truncating from {} to {}'.format(len(encoded_input['input_ids'][0]), max_retrieval_length))
                encoded_input = tokenizer(history_string, max_length=max_retrieval_length, truncation=True, return_tensors="pt")
                history_string = tokenizer.decode(encoded_input['input_ids'][0], skip_special_tokens=True)
        else:
            raise NotImplementedError
        prompt = answer_prompt_template.format(history_string, question_date_string, question_string)

    return prompt