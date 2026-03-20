import { type EnqueueResult, Logger, TriggerAction } from 'iii-sdk'
import { useApi } from './hooks'
import { iii } from './iii'

// --- Financial ledger: FIFO queue with per-account ordering ---

// In-memory account balances for demo purposes
const accounts = new Map<string, number>()

useApi(
  {
    api_path: 'transactions',
    http_method: 'POST',
    description: 'Submit a financial transaction to the FIFO ledger queue',
    metadata: { tags: ['queue', 'financial'] },
  },
  async (req, logger) => {
    const { account_id, type, amount } = req.body as {
      account_id: string
      type: 'deposit' | 'withdraw'
      amount: number
    }

    logger.info('Submitting transaction', { account_id, type, amount })

    const receipt = await iii.trigger<unknown, EnqueueResult>({
      function_id: 'ledger::apply',
      payload: { account_id, type, amount },
      action: TriggerAction.Enqueue({ queue: 'ledger' }),
    })

    return {
      status_code: 202,
      body: { receiptId: receipt.messageReceiptId },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)

// --- Worker: apply a ledger transaction (strictly ordered per account_id) ---

const ledgerLogger = new Logger(undefined, 'ledger::apply')

iii.registerFunction(
  { id: 'ledger::apply', metadata: { tags: ['queue', 'financial'] } },
  async payload => {
    const { account_id, type, amount } = payload as {
      account_id: string
      type: 'deposit' | 'withdraw'
      amount: number
    }

    const balance = accounts.get(account_id) ?? 0

    if (type === 'deposit') {
      accounts.set(account_id, balance + amount)
      ledgerLogger.info('Deposit applied', { account_id, amount, newBalance: balance + amount })
    } else if (type === 'withdraw') {
      if (balance < amount) {
        ledgerLogger.error('Insufficient funds', { account_id, balance, requested: amount })
        throw new Error('Insufficient funds')
      }
      accounts.set(account_id, balance - amount)
      ledgerLogger.info('Withdrawal applied', { account_id, amount, newBalance: balance - amount })
    }

    return { applied: true, account_id, newBalance: accounts.get(account_id) }
  },
)

// --- Read account balance ---

useApi(
  {
    api_path: 'accounts/:id/balance',
    http_method: 'GET',
    description: 'Get current account balance',
    metadata: { tags: ['queue', 'financial'] },
  },
  async (req, logger) => {
    const accountId = req.path_params.id
    const balance = accounts.get(accountId) ?? 0

    logger.info('Balance query', { accountId, balance })

    return {
      status_code: 200,
      body: { account_id: accountId, balance },
      headers: { 'Content-Type': 'application/json' },
    }
  },
)
