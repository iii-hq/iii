import { registerWorker, Logger } from "iii-sdk";

const iii = registerWorker(
  process.env.III_ENGINE_URL || "ws://localhost:49134",
  {
    workerName: "state-iii",
  },
);

iii.registerFunction({ id: "carts::add-item" }, async (request: any) => {
  const logger = new Logger();
  const cartId = request.params.cartId;
  const lineItem = await iii.trigger({
    function_id: "catalog-service::validate-line-item",
    payload: {
      sku: request.body.sku,
      qty: request.body.qty,
    },
  });
  const cart = await iii.trigger({
    function_id: "cart-service::add-item",
    payload: {
      cartId,
      item: lineItem,
    },
  });
  await iii.trigger({
    function_id: "state::set",
    payload: {
      scope: "carts",
      key: cartId,
      value: {
        _key: cartId,
        ...cart,
      },
    },
  });
  const quote = await iii.trigger({
    function_id: "pricing-service::quote-cart",
    payload: {
      cartId,
      items: cart.items,
    },
  });
  logger.info("state.cart_add_item", {
    cartId,
    sku: lineItem.sku,
  });
  return { cart, quote };
});

iii.registerFunction({ id: "carts::get" }, async (request: any) => {
  const logger = new Logger();
  let cart = await iii.trigger({
    function_id: "state::get",
    payload: {
      scope: "carts",
      key: request.params.cartId,
    },
  });
  if (!cart) {
    cart = await iii.trigger({
      function_id: "cart-service::get-cart",
      payload: {
        cartId: request.params.cartId,
      },
    });
    if (!cart) {
      logger.warn("state.cart_get.not_found", {
        cartId: request.params.cartId,
      });
      const error = new Error("Cart not found") as Error & {
        status: number;
      };
      error.status = 404;
      throw error;
    }
    await iii.trigger({
      function_id: "state::set",
      payload: {
        scope: "carts",
        key: request.params.cartId,
        value: {
          _key: request.params.cartId,
          ...cart,
        },
      },
    });
  }
  logger.info("state.cart_get.found", {
    cartId: request.params.cartId,
    itemCount: cart.items?.length ?? 0,
  });
  return cart;
});

iii.registerTrigger({
  type: "http",
  function_id: "carts::add-item",
  config: {
    api_path: "/state/carts/:cartId/items",
    http_method: "POST",
  },
});

iii.registerTrigger({
  type: "http",
  function_id: "carts::get",
  config: {
    api_path: "/state/carts/:cartId",
    http_method: "GET",
  },
});
